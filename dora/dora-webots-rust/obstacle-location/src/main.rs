use anyhow::Context;
use dora_node_api::{
    arrow::array::{Array, Float32Array, StructArray},
    dora_core::config::DataId,
    DoraNode, Event,
};
use nalgebra::{Matrix3, Matrix4, Vector4};
use std::env;
use std::error::Error;

mod utils;

const WIDTH: f32 = 1920.0;
const HEIGHT: f32 = 1080.0;
const FOV: f32 = 90.0;

// 标签映射函数
fn label_to_id(name: &str) -> f32 {
    match name {
        "car" => 1.0,
        "truck" => 2.0,
        "bus" => 3.0,
        "bicycle" => 4.0,
        "pedestrian" | "person" => 5.0,
        _ => 0.0,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // 【关键】注入 SUMO 环境变量，确保 Webots 控制器能找到 SUMO
    env::set_var(
        "SUMO_HOME",
        "/Library/Frameworks/EclipseSUMO.framework/Versions/Current/EclipseSUMO/share/sumo",
    );

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let intrinsic = utils::get_intrinsic_matrix(WIDTH, HEIGHT, FOV);

    let mut current_pc: Vec<[f32; 3]> = Vec::new();
    let mut camera_pc: Vec<[f32; 3]> = Vec::new();
    let mut extrinsic_matrix = Matrix4::<f32>::identity();

    let velodyne_to_camera = Matrix3::new(0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, metadata } => match id.as_str() {
                "lidar_pc" => {
                    let array = data
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .context("Lidar data is not Float32Array")?;

                    let raw_points = array.values();
                    current_pc = raw_points
                        .chunks_exact(3)
                        .map(|c| {
                            let p = nalgebra::Vector3::new(c[0], c[1], c[2]);
                            let transformed = velodyne_to_camera * p;
                            [transformed[0], transformed[1], transformed[2]]
                        })
                        // 优化过滤：排除车体自身点(1.0m内)以及高于路面的点(p[1]是高度)
                        .filter(|p| p[2] > 1.0 && p[2] < 60.0 && p[1] < 1.2)
                        .collect();

                    camera_pc = utils::project_to_camera(&current_pc, &intrinsic);
                }

                "position" => {
                    let array = data
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .context("Position data is not Float32Array")?;
                    extrinsic_matrix = utils::get_projection_matrix(array.values());
                }

                "obstacles_bbox" => {
                    let mut obstacles_3d = Vec::new();

                    // 如果点云还没准备好，发送空数据并跳过
                    if current_pc.is_empty() || camera_pc.is_empty() {
                        node.send_output(
                            DataId::from("obstacles".to_owned()),
                            metadata.parameters,
                            Float32Array::from(obstacles_3d),
                        )?;
                        continue;
                    }

                    let struct_array = data
                        .as_any()
                        .downcast_ref::<StructArray>()
                        .context("Input is not a StructArray")?;

                    let received_bboxes = utils::arrow_to_bboxes(struct_array)?;

                    for (name, rect, conf) in received_bboxes {
                        let min_x = rect.x as f32;
                        let max_x = (rect.x + rect.w) as f32;
                        let min_y = rect.y as f32;
                        let max_y = (rect.y + rect.h) as f32;

                        let mut pts_in_bbox: Vec<usize> = Vec::new();
                        for (i, cam_p) in camera_pc.iter().enumerate() {
                            // 增加深度约束，防止 BBox 误匹配背景噪点
                            if cam_p[0] > min_x
                                && cam_p[0] < max_x
                                && cam_p[1] > min_y
                                && cam_p[1] < max_y
                            {
                                pts_in_bbox.push(i);
                            }
                        }

                        if !pts_in_bbox.is_empty() {
                            // 取 1/4 分位数点，这是车辆后表面的稳健估计
                            pts_in_bbox.sort_by(|&a, &b| {
                                camera_pc[a][2].partial_cmp(&camera_pc[b][2]).unwrap()
                            });
                            let idx = pts_in_bbox[pts_in_bbox.len() / 4];
                            let local_pos = current_pc[idx];

                            let world_pos = extrinsic_matrix
                                * Vector4::new(local_pos[0], local_pos[1], local_pos[2], 1.0);

                            obstacles_3d.push(world_pos[0]);
                            obstacles_3d.push(world_pos[1]);
                            obstacles_3d.push(world_pos[2]);
                            obstacles_3d.push(conf);
                            obstacles_3d.push(label_to_id(&name)); // 存入对应的物体 ID
                        }
                    }

                    if !obstacles_3d.is_empty() {
                        println!(
                            "Detected {} obstacles from SUMO traffic",
                            obstacles_3d.len() / 5
                        );
                    }

                    node.send_output(
                        DataId::from("obstacles".to_owned()),
                        metadata.parameters,
                        Float32Array::from(obstacles_3d),
                    )?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}
