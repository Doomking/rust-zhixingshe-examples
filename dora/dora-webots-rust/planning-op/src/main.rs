use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use nalgebra::{Vector2, Vector3};
use std::error::Error;

#[derive(Debug, Clone)]
struct TrajectoryPoint {
    x: f32,
    y: f32,
    v: f32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    let mut current_pose = Vector3::new(0.0, 0.0, 0.0);
    let mut global_waypoints: Vec<Vector2<f32>> = Vec::new();
    let mut obstacles: Vec<Vector3<f32>> = Vec::new();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, metadata } => match id.as_str() {
                "position" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    let val = array.values();
                    // Webots: [x, y, z, rx, ry, rz] -> 平面坐标用 index 0 和 1
                    current_pose = Vector3::new(val[0], val[1], val[5]);
                }
                "objective_waypoints" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    // 这里应该包含 230-401 所有道路合并后的点
                    global_waypoints = array
                        .values()
                        .chunks_exact(2)
                        .map(|c| Vector2::new(c[0], c[1]))
                        .collect();
                    println!("Loaded {} global waypoints", global_waypoints.len());
                }
                "tick" => {
                    if global_waypoints.is_empty() {
                        continue;
                    }

                    // 执行轨迹规划
                    let target_path = plan_trajectory(&current_pose, &global_waypoints, &obstacles);

                    // 序列化发送
                    let mut output_path = Vec::new();
                    for p in &target_path {
                        output_path.push(p.x);
                        output_path.push(p.y);
                        output_path.push(p.v);
                    }

                    // 只有当路径有效时才发送
                    if !output_path.is_empty() {
                        node.send_output(
                            DataId::from("waypoints".to_owned()),
                            metadata.parameters,
                            Float32Array::from(output_path),
                        )?;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}

fn plan_trajectory(
    pose: &Vector3<f32>,
    waypoints: &[Vector2<f32>],
    _obstacles: &[Vector3<f32>],
) -> Vec<TrajectoryPoint> {
    let mut path = Vec::new();
    let lookahead_distance = 30.0; // 增加规划视野
    let target_speed = 2.0;

    // --- 1. 寻找最近点逻辑优化 ---
    // 增加一个距离阈值，如果离所有点都太远，则返回到起点的引导路径
    let mut min_dist = f32::MAX;
    let mut closest_idx = 0;
    for (i, wp) in waypoints.iter().enumerate() {
        let dist = (wp - Vector2::new(pose.x, pose.y)).norm();
        if dist < min_dist {
            min_dist = dist;
            closest_idx = i;
        }
    }

    // 调试打印：如果距离太远（比如 > 50m），发出警告
    if min_dist > 50.0 {
        println!(
            "WARNING: Vehicle is far from path (dist: {:.2}m). Finding path to start...",
            min_dist
        );
    }

    // --- 2. 动态提取路径切片 ---
    // 从最近点开始提取，直到覆盖足够的距离或点数
    let mut current_dist = 0.0;
    let max_points = 50; // 增加点数，让 control_op 有更平滑的曲线

    for i in closest_idx..waypoints.len() {
        let wp = waypoints[i];

        // 简单的速度规划：最后 10 米减速
        let dist_to_destination = (waypoints.len() - i) as f32;
        let speed = if dist_to_destination < 10.0 {
            1.0
        } else {
            target_speed
        };

        path.push(TrajectoryPoint {
            x: wp.x,
            y: wp.y,
            v: speed,
        });

        if i > closest_idx {
            current_dist += (waypoints[i] - waypoints[i - 1]).norm();
        }

        // 停止条件：距离够长 且 点数够多
        if current_dist > lookahead_distance && path.len() >= 20 {
            break;
        }
        if path.len() >= max_points {
            break;
        }
    }

    // --- 3. 兜底保护 ---
    // 如果已经在路径末尾，确保至少返回 2 个点，避免 control_op 崩溃
    if path.len() < 2 {
        let start = waypoints.len().saturating_sub(2);
        return waypoints[start..]
            .iter()
            .map(|wp| TrajectoryPoint {
                x: wp.x,
                y: wp.y,
                v: 0.5,
            })
            .collect();
    }

    path
}
