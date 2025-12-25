use dora_node_api::{
    arrow::array::{Float32Array, StringArray, UInt8Array},
    dora_core::config::DataId,
    DoraNode, Event, Parameter,
};
use std::error::Error;
use webots_sys::WebotsRobot; // 确保你的 WebotsRobot 结构体已经按之前的建议添加了解析方法

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    // 强制设置 Webots 控制器地址（如果在 Docker 或远程运行很有用）
    std::env::set_var("WEBOTS_CONTROLLER_URL", "tcp://127.0.0.1:1234");

    let robot = WebotsRobot::new();
    println!("Webots Bridge 优化版已启动...");

    // --- 核心修改：预先解析 Waypoints ---
    let road_ids = (230..=401)
        .map(|id| id.to_string())
        .collect::<Vec<String>>();
    let mut all_waypoints = Vec::new();

    for id in road_ids {
        let road_waypoints = robot.get_waypoints_from_opendrive(&id);
        if !road_waypoints.is_empty() {
            // 提取 [x, y, z] 并推入全局列表
            for pt in road_waypoints {
                // 这里 pt 通常包含 [x, y, v] 或 [x, y, z]
                // 确保你提取的是坐标信息
                all_waypoints.push(pt);
            }
        }
    }

    // 此时 all_waypoints 包含了从 230 到 401 的几千个点
    println!("Total global waypoints loaded: {}", all_waypoints.len());
    // 假设你想跟随 Road ID 为 "1" 的道路（这个 ID 需要根据你的 world_map.xodr 确定）
    let global_waypoints = all_waypoints;

    // 将 [ [x,y], [x,y] ] 展平为 [x1, y1, x2, y2, ...] 方便发送
    let waypoints_flat: Vec<f32> = global_waypoints.into_iter().flatten().collect();

    if waypoints_flat.is_empty() {
        println!("Warning: No waypoints extracted from Road ");
    } else {
        println!(
            "Successfully loaded {} waypoints from OpenDRIVE",
            waypoints_flat.len() / 2
        );
    }

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => match id.as_str() {
                "tick" => {
                    if !robot.step() {
                        break;
                    }

                    // 1. Camera Image
                    let image = robot.get_camera_image();
                    let w = robot.get_camera_width();
                    let h = robot.get_camera_height();
                    let mut params = metadata.parameters.clone();
                    params.insert("width".into(), Parameter::Integer(w as i64));
                    params.insert("height".into(), Parameter::Integer(h as i64));

                    node.send_output(
                        DataId::from("image".to_owned()),
                        params,
                        UInt8Array::from(image.to_vec()),
                    )?;

                    // 2. Lidar Point Cloud
                    let pc = robot.get_lidar_points();
                    node.send_output(
                        DataId::from("lidar_pc".to_owned()),
                        metadata.parameters.clone(),
                        Float32Array::from(pc),
                    )?;

                    // 3. Position & Attitude (6DOF: x, y, z, r, p, y)
                    let pos = robot.get_gps_position();
                    let att = robot.get_attitude();
                    let full_pose = vec![pos[0], pos[1], pos[2], att[0], att[1], att[2]];
                    node.send_output(
                        DataId::from("position".to_owned()),
                        metadata.parameters.clone(),
                        Float32Array::from(full_pose),
                    )?;

                    println!("webots-bridge gps pos: {:?}", pos);
                    // 4. Speed
                    let speed = vec![robot.get_speed()];
                    node.send_output(
                        DataId::from("speed".to_owned()),
                        metadata.parameters.clone(),
                        Float32Array::from(speed),
                    )?;

                    // 5. OpenDrive Raw Data
                    let opendrive = robot.get_opendrive();
                    node.send_output(
                        DataId::from("opendrive".to_owned()),
                        metadata.parameters.clone(),
                        StringArray::from(vec![opendrive]),
                    )?;

                    // --- 6. 修改：发送解析后的 Objective Waypoints ---
                    // 这里直接发送预解析好的全局路点
                    // 下游控制节点（如 PID）会结合当前的 position 算出局部目标
                    node.send_output(
                        DataId::from("objective_waypoints".to_owned()),
                        metadata.parameters.clone(),
                        Float32Array::from(waypoints_flat.clone()),
                    )?;
                }
                // 在 webots_bridge 的 main 循环中添加对 control_command 的处理
                "control_command" => {
                    let array = data.as_any().downcast_ref::<Float32Array>().unwrap();
                    let cmd = array.values(); // [steering, throttle, brake]

                    println!("control_command: {:?}", cmd);

                    let steering = cmd[0] as f64;
                    let throttle = cmd[1] as f64;
                    let brake = cmd[2] as f64;

                    // 应用到 Webots 电机
                    robot.set_steering(steering);

                    // 简单的动力模型：速度 = 油门 * 最大速度 - 刹车系数
                    let target_vel = if brake > 0.1 {
                        0.0
                    } else {
                        throttle * 120.0 // 假设最大车速 20m/s
                    };
                    println!("webots-bridge: speed: {}", target_vel);
                    robot.set_drive_speed(target_vel);
                }
                other => eprintln!("Received unknown input: {:?}", other),
            },
            _ => {}
        }
    }

    Ok(())
}
