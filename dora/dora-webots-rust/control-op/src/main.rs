use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use std::error::Error;

const WHEEL_BASE: f32 = 2.94;
const MAX_STEER_ANGLE: f32 = 0.55; // 进一步压低，彻底消除 Webots 1.0 报错
const MAX_STEER_STEP: f32 = 0.15; // 减缓变化率，增加稳定性

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut current_pose = [0.0f32; 6];
    let mut planned_path: Vec<[f32; 3]> = Vec::new();
    let mut last_steering = 0.0f32;
    // 增加：调头方向锁定标志
    let mut uturn_lock: f32 = 0.0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, metadata } => match id.as_str() {
                "position" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    current_pose.copy_from_slice(array.values());
                }
                "waypoints" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    planned_path = array
                        .values()
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                }
                "tick" => {
                    if planned_path.is_empty() {
                        continue;
                    }

                    let x = current_pose[0];
                    let y = current_pose[1];
                    let yaw = current_pose[5];

                    let (closest_idx, min_dist) = planned_path
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i, ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()))
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .unwrap();

                    let mut target_speed: f32 = 4.0;
                    let target_steer: f32;
                    let ld = if min_dist > 10.0 {
                        6.0f32.max(min_dist * 0.4).min(15.0)
                    } else {
                        (last_steering.abs() * -8.0 + 10.0).clamp(3.5, 10.0)
                    };

                    let target_pt_idx = planned_path[closest_idx..]
                        .iter()
                        .position(|p| ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() > ld)
                        .map(|i| i + closest_idx)
                        .unwrap_or(planned_path.len() - 1);
                    let target_pt = planned_path[target_pt_idx];

                    let target_angle = (target_pt[1] - y).atan2(target_pt[0] - x);
                    let mut alpha = target_angle - yaw;
                    while alpha > std::f32::consts::PI {
                        alpha -= 2.0 * std::f32::consts::PI;
                    }
                    while alpha < -std::f32::consts::PI {
                        alpha += 2.0 * std::f32::consts::PI;
                    }

                    // === 改进后的状态机逻辑 ===
                    if alpha.abs() > 2.0 {
                        // 1. 进入/保持调头模式
                        if uturn_lock == 0.0 {
                            // 第一次识别到背向，锁定当前最优转向方向
                            uturn_lock = if alpha > 0.0 { 1.0 } else { -1.0 };
                        }
                        target_steer = uturn_lock * MAX_STEER_ANGLE;
                        target_speed = 3.5;
                        print!("[UTURN-LOCKED] ");
                    } else if alpha.abs() < 0.5 {
                        // 2. 只有角度很小时，才解除掉头锁定
                        uturn_lock = 0.0;
                        let curvature = 2.0 * alpha.sin() / ld;
                        target_steer = (curvature * WHEEL_BASE).atan();
                    } else if uturn_lock != 0.0 {
                        // 3. 还在调头过程中，即使 alpha 变小了也继续转，直到 alpha < 0.5
                        target_steer = uturn_lock * MAX_STEER_ANGLE;
                        target_speed = 4.0;
                        print!("[UTURN-FINISHING] ");
                    } else {
                        // 4. 正常循迹
                        let curvature = 2.0 * alpha.sin() / ld;
                        target_steer = (curvature * WHEEL_BASE).atan();
                        target_speed = if min_dist > 10.0 {
                            10.0
                        } else {
                            planned_path[closest_idx][2]
                        };
                    }

                    // 最终输出限位与平滑
                    let clamped_steer = target_steer.clamp(-MAX_STEER_ANGLE, MAX_STEER_ANGLE);
                    let final_steer = last_steering
                        + (clamped_steer - last_steering).clamp(-MAX_STEER_STEP, MAX_STEER_STEP);
                    last_steering = final_steer;

                    let final_speed = if final_steer.abs() > 0.4 {
                        target_speed.min(3.5)
                    } else {
                        target_speed
                    };

                    println!(
                        "Dist: {:.1}m, Alpha: {:.2}, Steer: {:.2}, V: {:.1}",
                        min_dist, alpha, final_steer, final_speed
                    );

                    node.send_output(
                        DataId::from("control_command".to_owned()),
                        metadata.parameters.clone(),
                        Float32Array::from(vec![final_steer, final_speed]),
                    )?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}
