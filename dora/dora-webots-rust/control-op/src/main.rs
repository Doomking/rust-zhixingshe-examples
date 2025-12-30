use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use std::error::Error;

const WHEEL_BASE: f32 = 2.94;
const MAX_STEER_ANGLE: f32 = 0.55;

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut current_pose = [0.0f32; 6];
    let mut planned_path: Vec<[f32; 3]> = Vec::new();
    let mut last_steering = 0.0f32;
    let mut uturn_lock: f32 = 0.0;
    // let mut last_closest_idx: usize = 0;

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
                    // last_closest_idx = 0;
                }
                "tick" => {
                    if planned_path.is_empty() {
                        continue;
                    }

                    let x = current_pose[0];
                    let y = current_pose[1];
                    let yaw = current_pose[5];

                    // 1. 全局搜索最近点（去掉局部搜索，防止掉头时索引卡死）
                    let (closest_idx, min_dist) = planned_path
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i, ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()))
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .unwrap();
                    // last_closest_idx = closest_idx;

                    // 2. 动态预瞄距离
                    let ld = if min_dist > 20.0 { 3.0f32 } else { 6.0f32 };

                    // 3. 计算 Alpha
                    let target_pt = planned_path[closest_idx]; // 掉头时直接瞄准最近点，确保转弯半径最小
                    let target_angle = (target_pt[1] - y).atan2(target_pt[0] - x);
                    let mut alpha = target_angle - yaw;
                    while alpha > std::f32::consts::PI {
                        alpha -= 2.0 * std::f32::consts::PI;
                    }
                    while alpha < -std::f32::consts::PI {
                        alpha += 2.0 * std::f32::consts::PI;
                    }

                    let target_speed: f32;
                    let target_steer: f32;

                    // 4. 核心状态机逻辑优化
                    if alpha.abs() > 1.2 {
                        // 【掉头模式】
                        if uturn_lock == 0.0 {
                            uturn_lock = if alpha > 0.0 { 1.0 } else { -1.0 };
                        }
                        target_steer = uturn_lock * MAX_STEER_ANGLE;
                        // 关键：大幅降低掉头速度，防止转圈过猛
                        target_speed = 1.2;
                    } else if uturn_lock != 0.0 {
                        // 【掉头保持模式】
                        if alpha.abs() < 0.3 {
                            uturn_lock = 0.0; // 角度很小时才解除锁定
                            target_steer = 0.0;
                        } else {
                            target_steer = uturn_lock * MAX_STEER_ANGLE * 0.8;
                        }
                        target_speed = 2.0;
                    } else {
                        // 【正常循迹模式】
                        let curvature = 2.0 * alpha.sin() / ld;
                        target_steer = (curvature * WHEEL_BASE).atan();
                        target_speed = 4.0;
                    }

                    // 5. 转向处理（解决右转过快问题）
                    let mut final_steer = target_steer.clamp(-MAX_STEER_ANGLE, MAX_STEER_ANGLE);

                    // --- 针对右转快了的专项修正 ---
                    // 如果 Steering > 0 (右转)，对其进行缩放抑制
                    if final_steer > 0.0 {
                        // 方案：使用非线性缩减，角度越大抑制越强
                        final_steer *= 0.199;
                    }
                    // ----------------------------

                    // 6. 步进平滑（防止Webots物理震荡）
                    let max_step = 0.1;
                    final_steer =
                        last_steering + (final_steer - last_steering).clamp(-max_step, max_step);

                    let mut final_speed = target_speed;
                    // 保护逻辑：转向角大时强制限速
                    if final_steer.abs() > 0.3 {
                        final_speed = final_speed.min(1.5);
                    }

                    last_steering = final_steer;

                    println!(
                        "Dist: {:.1}m, Alpha: {:.2}, Steer: {:.2}, V: {:.1}, Lock: {}",
                        min_dist, alpha, final_steer, final_speed, uturn_lock
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
