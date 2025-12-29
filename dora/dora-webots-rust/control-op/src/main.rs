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
    let mut last_closest_idx: usize = 0;

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
                    last_closest_idx = 0;
                }
                "tick" => {
                    if planned_path.is_empty() {
                        continue;
                    }

                    let x = current_pose[0];
                    let y = current_pose[1];
                    let yaw = current_pose[5];

                    // 1. 局部搜索最近点
                    let search_start = last_closest_idx.saturating_sub(20);
                    let search_end = (last_closest_idx + 200).min(planned_path.len());

                    let (closest_idx, min_dist) = planned_path[search_start..search_end]
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            (
                                i + search_start,
                                ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt(),
                            )
                        })
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .unwrap();

                    last_closest_idx = closest_idx;

                    // 2. 动态预瞄距离 (ld)
                    // 如果距离路径极远，强制减小 ld 以增大转向角
                    let ld = if min_dist > 40.0 {
                        3.0 // 强迫车辆急转
                    } else if min_dist > 10.0 {
                        (8.0 + min_dist * 0.1).min(15.0)
                    } else {
                        (last_steering.abs() * -4.0 + 8.0).clamp(4.5, 8.0)
                    };

                    // 3. 寻找目标点
                    let target_pt_idx = planned_path[closest_idx..]
                        .iter()
                        .enumerate()
                        .position(|(_, p)| ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() > ld)
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

                    let mut target_speed: f32;
                    let target_steer: f32;

                    // 4. 核心状态机逻辑
                    if alpha.abs() > 1.6 {
                        // 【强制调头模式】夹角过大时，无视 Pure Pursuit，直接打死
                        if uturn_lock == 0.0 {
                            uturn_lock = if alpha > 0.0 { 1.0 } else { -1.0 };
                        }
                        target_steer = uturn_lock * MAX_STEER_ANGLE * 1.0; // 满打转向
                        target_speed = 3.8; // 降低速度以获得更小的物理转弯半径
                    } else if uturn_lock != 0.0 {
                        // 【调头保持/回正模式】
                        // 增加判定：如果 alpha 还是很大 (>0.8)，继续维持高强度转向，不急着回正
                        if alpha.abs() < 0.4 {
                            uturn_lock = 0.0; // 只有对得比较准了才释放锁定
                            let curvature = 2.0 * alpha.sin() / ld;
                            target_steer = (curvature * WHEEL_BASE).atan();
                        } else {
                            // 依然在弯道中，维持 90% 的转向力，防止回正过早导致压外线
                            target_steer = uturn_lock * MAX_STEER_ANGLE * 0.95;
                        }
                        target_speed = 4.5;
                    } else {
                        // 【正常循迹模式】
                        let curvature = 2.0 * alpha.sin() / ld;
                        target_steer = (curvature * WHEEL_BASE).atan();
                        target_speed = if min_dist > 5.0 {
                            6.0
                        } else {
                            planned_path[closest_idx][2]
                        };
                    }

                    // 5. 转向平滑与限速
                    let max_step = 0.15; // 提高步进响应速度
                    let clamped_steer = target_steer.clamp(-MAX_STEER_ANGLE, MAX_STEER_ANGLE);
                    let mut final_steer =
                        last_steering + (clamped_steer - last_steering).clamp(-max_step, max_step);

                    // --- 专门针对右转角度调小的逻辑 ---
                    let mut adjusted_steer = final_steer;

                    // 假设在你的系统中，正值代表右转 (Positive = Right)
                    // 如果实际运行发现左转变小了，请把 > 改为 <
                    if adjusted_steer > 0.0 {
                        adjusted_steer *= 0.5; // 这里 0.8 代表只保留 80% 的转向力度，你可以根据需要调整
                    }

                    // 确保调整后依然在物理限值内
                    final_steer = adjusted_steer.clamp(-MAX_STEER_ANGLE, MAX_STEER_ANGLE);
                    // ------------------------------

                    let mut final_speed = target_speed;
                    if final_steer.abs() > 0.4 {
                        final_speed = final_speed.min(4.0);
                    }
                    if min_dist > 30.0 {
                        final_speed = final_speed.min(5.5); // 确保远距离回归时不会因为速度过快冲出去
                    }

                    last_steering = final_steer;

                    println!(
                        "Idx: {}, Dist: {:.1}m, Alpha: {:.2}, Steer: {:.2}, V: {:.1}, Ld: {:.1}",
                        closest_idx, min_dist, alpha, final_steer, final_speed, ld
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
