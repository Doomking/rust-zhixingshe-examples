use anyhow::{Context, Result};
use dora_node_api::{
    arrow::array::{Float32Array, StructArray, UInt8Array},
    DoraNode, Event, Parameter,
};
use opencv::{
    core::{AlgorithmHint, Mat, Point, Rect, Scalar, Vec4b},
    highgui, imgproc,
    prelude::*,
};
use std::error::Error;

mod utils;
use utils::arrow_to_bboxes;

fn main() -> Result<(), Box<dyn Error>> {
    // 1. 初始化 dora 节点
    let (mut _node, mut events) = DoraNode::init_from_env()?;

    // --- 状态缓存 (用于多传感器数据融合渲染) ---
    let mut bboxes = Vec::new(); // YOLO 2D 框
    let mut current_position = [0.0f32; 6]; // [x, y, z, roll, pitch, yaw]
    let mut current_speed = 0.0f32; // 实时速度
    let mut planned_path = Vec::new(); // 规划路径点 [x, z]
    let mut obstacles_3d = Vec::new(); // 障碍物世界坐标 [x, z]

    // 创建 OpenCV 窗口
    let win_name = "Dora Autonomous Driving Monitor";
    highgui::named_window(win_name, highgui::WINDOW_NORMAL)
        .context("Failed to create highgui window")?;

    println!("Plot operator initialized. Waiting for data...");

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => match id.as_str() {
                // --- A. 接收 YOLO 检测框 ---
                "detections" => {
                    if let Some(struct_array) = data.as_any().downcast_ref::<StructArray>() {
                        if let Ok(received_bboxes) = arrow_to_bboxes(struct_array) {
                            bboxes = received_bboxes;
                        }
                    }
                }

                // --- B. 接收 3D 障碍物位置 ---
                "obstacles" => {
                    if let Some(array) = data.as_any().downcast_ref::<Float32Array>() {
                        // 格式: [x, y, z, conf, label, ...]
                        obstacles_3d = array
                            .values()
                            .chunks_exact(5)
                            .map(|c| [c[0], c[2]]) // 提取 X 和 Z
                            .collect();
                    }
                }

                // --- C. 接收自车位姿 ---
                "position" => {
                    if let Some(array) = data.as_any().downcast_ref::<Float32Array>() {
                        if array.len() >= 6 {
                            current_position.copy_from_slice(array.values());
                        }
                    }
                }

                // --- D. 接收规划路径 ---
                "waypoints" => {
                    if let Some(array) = data.as_any().downcast_ref::<Float32Array>() {
                        // 格式: [x, y, v, ...]
                        println!("Received {} waypoints", array.len() / 3);
                        planned_path = array
                            .values()
                            .chunks_exact(3)
                            .map(|c| [c[0], c[1]])
                            .collect();
                    }
                }

                // --- E. 接收控制信息 (获取速度) ---
                "control" => {
                    if let Some(array) = data.as_any().downcast_ref::<Float32Array>() {
                        // 假设 control 发送的是 [steering, throttle, brake]
                        // 我们从这里或专门的 speed 节点获取速度
                    }
                }

                // --- F. 核心渲染逻辑：处理图像输入 ---
                "frame" => {
                    let cols = match metadata.parameters.get("width") {
                        Some(Parameter::Integer(v)) => *v as i32,
                        _ => 640,
                    };
                    let rows = match metadata.parameters.get("height") {
                        Some(Parameter::Integer(v)) => *v as i32,
                        _ => 480,
                    };

                    let uint8_array = data
                        .as_any()
                        .downcast_ref::<UInt8Array>()
                        .context("Expected UInt8Array for image")?;
                    let byte_slice = uint8_array.values();

                    if byte_slice.len() != (rows * cols * 4) as usize {
                        continue;
                    }

                    // 1. BGRA 转 BGR
                    let (_head, vec4_slice, _tail) = unsafe { byte_slice.align_to::<Vec4b>() };
                    let frame_raw = Mat::new_rows_cols_with_data(rows, cols, vec4_slice)?;
                    let mut display_frame = Mat::default();
                    imgproc::cvt_color(
                        &frame_raw,
                        &mut display_frame,
                        imgproc::COLOR_BGRA2BGR,
                        0,
                        AlgorithmHint::ALGO_HINT_DEFAULT,
                    )?;

                    // 2. 绘制 YOLO 2D 检测框
                    for (classname, bbox, conf) in &bboxes {
                        imgproc::rectangle(
                            &mut display_frame,
                            *bbox,
                            Scalar::new(0.0, 255.0, 0.0, 0.0),
                            2,
                            8,
                            0,
                        )?;
                        let label = format!("{}: {:.2}", classname, conf);
                        imgproc::put_text(
                            &mut display_frame,
                            &label,
                            Point::new(bbox.x, bbox.y - 5),
                            imgproc::FONT_HERSHEY_SIMPLEX,
                            0.5,
                            Scalar::new(0.0, 255.0, 0.0, 0.0),
                            1,
                            8,
                            false,
                        )?;
                    }

                    // 3. 绘制 HUD 仪表盘文本
                    let hud_color = Scalar::new(255.0, 255.0, 255.0, 0.0);
                    let pos_info = format!(
                        "GPS: X:{:.1} Z:{:.1} Yaw:{:.2}",
                        current_position[0], current_position[2], current_position[5]
                    );
                    imgproc::put_text(
                        &mut display_frame,
                        &pos_info,
                        Point::new(20, 30),
                        imgproc::FONT_HERSHEY_SIMPLEX,
                        0.6,
                        hud_color,
                        2,
                        8,
                        false,
                    )?;

                    // 4. 绘制 2D 小地图 (Bird's Eye View)
                    let map_size = 200;
                    let map_rect = Rect::new(cols - map_size - 20, 20, map_size, map_size);
                    // 背景
                    // imgproc::rectangle(
                    //     &mut display_frame,
                    //     map_rect,
                    //     Scalar::new(30.0, 30.0, 30.0, 0.0),
                    //     -1,
                    //     8,
                    //     0,
                    // )?;
                    let center = Point::new(map_rect.x + map_size / 2, map_rect.y + map_size / 2);

                    // 绘制 3D 障碍物点 (红色点)
                    for obs in &obstacles_3d {
                        let dx = (obs[0] - current_position[0]) * 5.0; // 比例尺: 1m = 5px
                        let dz = (obs[1] - current_position[2]) * 5.0;
                        let p = Point::new(center.x + dx as i32, center.y - dz as i32);
                        if map_rect.contains(p) {
                            imgproc::circle(
                                &mut display_frame,
                                p,
                                3,
                                Scalar::new(0.0, 0.0, 255.0, 0.0),
                                -1,
                                8,
                                0,
                            )?;
                        }
                    }

                    // 绘制规划路径 (青色点序列)
                    for wp in &planned_path {
                        let dx = (wp[0] - current_position[0]) * 5.0;
                        let dz = (wp[1] - current_position[2]) * 5.0;
                        let p = Point::new(center.x + dx as i32, center.y - dz as i32);
                        if map_rect.contains(p) {
                            println!("viewer waypoint: {:?}", p);
                            // 画一个亮黄色的点代表目标
                            imgproc::circle(
                                &mut display_frame,
                                p,
                                5,
                                Scalar::new(0.0, 255.0, 255.0, 0.0),
                                -1,
                                8,
                                0,
                            )?;
                            // imgproc::circle(
                            //     &mut display_frame,
                            //     p,
                            //     1,
                            //     Scalar::new(255.0, 255.0, 0.0, 0.0),
                            //     -1,
                            //     8,
                            //     0,
                            // )?;
                        }
                    }

                    // 绘制自车位置 (白色中心点)
                    // imgproc::circle(
                    //     &mut display_frame,
                    //     center,
                    //     4,
                    //     Scalar::new(255.0, 255.0, 255.0, 0.0),
                    //     -1,
                    //     8,
                    //     0,
                    // )?;

                    // 5. 显示并刷新
                    highgui::imshow(win_name, &display_frame)?;
                    if highgui::wait_key(1)? == 27 {
                        break;
                    } // ESC 退出
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}
