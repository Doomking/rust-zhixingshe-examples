use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use nalgebra::{Vector2, Vector3};
use std::error::Error;

struct TrajectoryPoint {
    x: f32,
    y: f32,
    v: f32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    let mut current_pose = Vector3::new(0.0, 0.0, 0.0);
    let mut global_waypoints: Vec<Vector2<f32>> = Vec::new();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, metadata } => match id.as_str() {
                "position" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    let val = array.values();
                    current_pose = Vector3::new(val[0], val[1], val[5]);
                }
                "objective_waypoints" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
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

                    let mut path = Vec::new();
                    let car_pos = Vector2::new(current_pose.x, current_pose.y);

                    // 1. 找最近点
                    let (closest_idx, min_dist) = global_waypoints
                        .iter()
                        .enumerate()
                        .map(|(i, wp)| (i, (wp - car_pos).norm()))
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .unwrap();

                    // 2. 速度决策
                    let is_far = min_dist > 5.0;
                    let capture_speed = 12.0f32; // 追路速度
                    let cruise_speed = 22.0f32; // 巡航速度 (~80km/h)

                    // 3. 生成局部路径片段
                    let lookahead_dist = 50.0;
                    let mut accumulated_dist = 0.0;

                    for i in closest_idx..global_waypoints.len() {
                        let wp = global_waypoints[i];
                        let remaining = global_waypoints.len() - i;

                        // 动态速度规划
                        let v = if is_far {
                            capture_speed
                        } else if remaining < 15 {
                            5.0 // 终点前速度从3.0提高到5.0
                        } else if remaining < 40 {
                            15.0 // 接近终点速度从10.0提高到15.0
                        } else {
                            cruise_speed
                        };

                        path.push(TrajectoryPoint {
                            x: wp.x,
                            y: wp.y,
                            v,
                        });

                        if i > closest_idx {
                            accumulated_dist +=
                                (global_waypoints[i] - global_waypoints[i - 1]).norm();
                        }
                        if accumulated_dist > lookahead_dist || path.len() >= 30 {
                            break;
                        }
                    }

                    // 发送数据
                    let mut output = Vec::new();
                    for p in path {
                        output.push(p.x);
                        output.push(p.y);
                        output.push(p.v);
                    }
                    node.send_output(
                        DataId::from("waypoints".to_owned()),
                        metadata.parameters,
                        Float32Array::from(output),
                    )?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}
