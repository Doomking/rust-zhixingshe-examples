use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use std::error::Error;

struct PIDController {
    k_p: f32,
    k_i: f32,
    k_d: f32,
    integral: f32,
    previous_error: f32,
    max_integral: f32,
}

impl PIDController {
    fn new(k_p: f32, k_i: f32, k_d: f32) -> Self {
        Self {
            k_p,
            k_i,
            k_d,
            integral: 0.0,
            previous_error: 0.0,
            max_integral: 1.0,
        }
    }

    fn compute(&mut self, setpoint: f32, measured: f32, dt: f32) -> f32 {
        let error = setpoint - measured;
        self.integral += error * dt;
        self.integral = self.integral.clamp(-self.max_integral, self.max_integral);
        let derivative = (error - self.previous_error) / dt;
        self.previous_error = error;
        (self.k_p * error) + (self.k_i * self.integral) + (self.k_d * derivative)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut speed_pid = PIDController::new(0.2, 0.005, 0.1);
    let wheel_base = 2.94;
    let steering_ratio = 25.0;

    let mut current_pose = [0.0f32; 6];
    let mut current_speed = 0.0f32;
    let mut planned_path: Vec<[f32; 3]> = Vec::new();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, metadata } => match id.as_str() {
                "position" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    current_pose.copy_from_slice(array.values());
                }
                "speed" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    current_speed = array.values()[0].abs();
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

                    // --- 1. 寻找最近路径点索引 ---
                    let mut min_dist = f32::MAX;
                    let mut closest_idx = 0;
                    for (i, p) in planned_path.iter().enumerate() {
                        let dist = ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt();
                        if dist < min_dist {
                            min_dist = dist;
                            closest_idx = i;
                        }
                    }

                    // --- 2. 动态预瞄 (根据当前位置计算) ---
                    let lookahead_dist = (current_speed * 0.5).max(5.0).min(15.0);
                    let target_pt = planned_path[closest_idx..]
                        .iter()
                        .find(|p| ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() > lookahead_dist)
                        .unwrap_or(&planned_path[planned_path.len() - 1]);

                    // --- 3. 转向逻辑优化 ---
                    let dx = target_pt[0] - x;
                    let dy = target_pt[1] - y;

                    // 标准右手系转换 (Z-up)
                    let local_x = dx * yaw.cos() + dy * yaw.sin();
                    let local_y = -dx * yaw.sin() + dy * yaw.cos();

                    let angle_to_target = local_y.atan2(local_x);

                    // 增加防震荡系数：如果偏离过远（min_dist > 10m），降低转向增益
                    let gain_scale = if min_dist > 10.0 { 0.5 } else { 1.0 };
                    let steer_wheel_angle =
                        (2.0 * wheel_base * angle_to_target.sin() / lookahead_dist).atan();
                    let mut final_steering = steer_wheel_angle * steering_ratio * gain_scale;

                    final_steering = final_steering.clamp(-10.4, 10.4);

                    // --- 4. 纵向速度控制 (解决油门制动冲突) ---
                    let target_speed = target_pt[2];
                    let control_effort = speed_pid.compute(target_speed, current_speed, 0.05);

                    // 转向补偿油门
                    let steering_resistance = (final_steering.abs() / 10.4) * 0.2;
                    let (throttle, brake) = if control_effort > 0.05 {
                        (
                            (control_effort + 0.15 + steering_resistance).clamp(0.0, 0.6),
                            0.0,
                        )
                    } else if control_effort < -0.05 {
                        (0.0, (-control_effort).clamp(0.0, 1.0))
                    } else {
                        (0.0, 0.0) // 死区保护
                    };

                    // --- 5. 输出 ---
                    node.send_output(
                        DataId::from("control_command".to_owned()),
                        metadata.parameters,
                        Float32Array::from(vec![final_steering, throttle, brake]),
                    )?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}
