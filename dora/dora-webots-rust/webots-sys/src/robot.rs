use super::*;
use roxmltree;
use std::env;
use std::ffi::CString;

// devices list: 37

//  Device Index 0: left_steer

//  Device Index 1: left_steer_sensor

//  Device Index 2: left_front_wheel

//  Device Index 3: left_front_sensor

//  Device Index 4: left_front_brake

//  Device Index 5: right_steer

//  Device Index 6: right_steer_sensor

//  Device Index 7: right_front_wheel

//  Device Index 8: right_front_sensor

//  Device Index 9: right_front_brake

//  Device Index 10: left_rear_sensor

//  Device Index 11: left_rear_brake

//  Device Index 12: right_rear_sensor

//  Device Index 13: right_rear_brake

//  Device Index 14: engine_speaker

//  Device Index 15: camera

//  Device Index 16: gps

//  Device Index 17: gyro

//  Device Index 18: display

//  Device Index 19: front_lights

//  Device Index 20: right_indicators

//  Device Index 21: left_indicators

//  Device Index 22: antifog_lights

//  Device Index 23: brake_lights

//  Device Index 24: rear_lights

//  Device Index 25: backwards_lights

//  Device Index 26: interior_right_indicators

//  Device Index 27: interior_left_indicators

//  Device Index 28: right_wiper_motor

//  Device Index 29: wiper_sensor

//  Device Index 30: left_wiper_motor

//  Device Index 31: indicator_lever_motor

//  Device Index 32: rear_yaw_mirror_frame_motor

//  Device Index 33: rear_pitch_mirror_frame_motor

//  Device Index 34: steering_wheel_motor

//  Device Index 35: speed_needle_motor

//  Device Index 36: rpm_needle_motor

pub struct WebotsRobot {
    pub time_step: i32,
    pub camera: WbDeviceTag,
    pub lidar: WbDeviceTag,
    pub gps: WbDeviceTag,
    // 改为存储所有驱动轮，确保动力充足
    pub drive_motors: Vec<WbDeviceTag>,
    pub steering_wheel: WbDeviceTag,
    pub brakes: Vec<WbDeviceTag>,
    pub imu: WbDeviceTag,
    pub opendrive_data: String,
}

impl WebotsRobot {
    pub fn get_device_list() {
        unsafe {
            let count = wb_robot_get_number_of_devices();
            println!("Total devices found: {}", count);
            for i in 0..count {
                let tag = wb_robot_get_device_by_index(i);

                // 尝试这个最原始的 C 函数名
                let name_ptr = wb_device_get_name(tag);

                if !name_ptr.is_null() {
                    let name = std::ffi::CStr::from_ptr(name_ptr).to_string_lossy();
                    println!("Device Index {}: {}", i, name);
                }
            }
        }
    }
    pub fn new() -> Self {
        unsafe {
            wb_robot_init();
            let time_step = wb_robot_get_basic_time_step() as i32;

            // WebotsRobot::get_device_list();

            // 1. 基础传感器 (保持不变)
            let camera = Self::get_device("camera");
            if camera != 0 {
                wb_camera_enable(camera, time_step);
            }
            // let lidar = Self::get_device("lidar"); // 如果名单里没有，这步会返回 0
            // if lidar != 0 { wb_lidar_enable(lidar, time_step); }
            let gps = Self::get_device("gps");
            if gps != 0 {
                wb_gps_enable(gps, time_step);
            }

            // 2. 初始化驱动电机 (全轮驱动)
            // 根据你的名单，虽然只列出了 front，但后轮电机通常隐藏在 API 命名规律中
            let motor_names = ["left_front_wheel", "right_front_wheel"];
            let mut drive_motors = Vec::new();
            for name in motor_names {
                let motor = Self::get_device(name);
                if motor != 0 {
                    wb_motor_set_position(motor, f64::INFINITY);
                    wb_motor_set_velocity(motor, 100.0);
                    drive_motors.push(motor);
                }
            }

            // 3. 初始化转向电机
            let steering_wheel = Self::get_device("steering_wheel_motor");
            if steering_wheel != 0 {
                wb_motor_set_position(steering_wheel, 0.0);
            }

            // 4. 彻底释放刹车 (Brakes)
            let brake_names = [
                "left_front_brake",
                "right_front_brake",
                "left_rear_brake",
                "right_rear_brake",
            ];
            let mut brakes = Vec::new();
            for name in brake_names {
                let tag = Self::get_device(name);
                if tag != 0 {
                    // 关键：将阻尼设为 0，防止物理阻力
                    wb_brake_set_damping_constant(tag, 0.0);
                    brakes.push(tag);
                }
            }

            // --- Lidar 初始化 ---
            let lidar = Self::get_device("lidar"); // 确保 Webots 里设备名叫 "lidar"
            if lidar != 0 {
                wb_lidar_enable(lidar, time_step);
                // 【关键步骤】必须显式开启点云生成，否则 get_point_cloud 会返回 null
                wb_lidar_enable_point_cloud(lidar);
            }

            let imu = Self::get_device("inertial unit"); // 注意：请检查你名单里的实际名称，有时叫 "imu"
            if imu != 0 {
                unsafe {
                    wb_inertial_unit_enable(imu, time_step);
                }
            }

            // 尝试加载 OpenDrive 文件（确保该文件已放在项目根目录）
            let current_dir = env::current_dir().expect("Failed to get current working directory");
            let relative_path = "webots-sys/data/sumo_map.xodr";
            let path = current_dir.join(relative_path);
            let opendrive_data = std::fs::read_to_string(path).unwrap_or_else(|_| {
                println!("Warning: world_map.xodr not found. Using empty string.");
                "".to_string()
            });

            println!(
                "Initialized: {} motors, {} brakes, {} lidar, {} imu, {} opendrive.",
                drive_motors.len(),
                brakes.len(),
                lidar,
                imu,
                "",
            );

            Self {
                time_step,
                camera,
                lidar,
                gps,
                drive_motors,
                steering_wheel,
                brakes,
                imu,
                opendrive_data,
            }
        }
    }

    fn get_device(name: &str) -> WbDeviceTag {
        let c_name = CString::new(name).expect("Invalid device name");
        unsafe { wb_robot_get_device(c_name.as_ptr()) }
    }

    pub fn step(&self) -> bool {
        unsafe { wb_robot_step(self.time_step) != -1 }
    }

    /// 获取图像原始字节流
    pub fn get_camera_image(&self) -> &[u8] {
        unsafe {
            let w = wb_camera_get_width(self.camera);
            let h = wb_camera_get_height(self.camera);
            let ptr = wb_camera_get_image(self.camera);
            std::slice::from_raw_parts(ptr, (w * h * 4) as usize)
        }
    }

    pub fn get_camera_width(&self) -> i32 {
        unsafe { wb_camera_get_width(self.camera) }
    }

    pub fn get_camera_height(&self) -> i32 {
        unsafe { wb_camera_get_height(self.camera) }
    }

    // 获取 GPS 坐标 [x, y, z] - 已修复指针错误
    pub fn get_gps_position(&self) -> [f32; 3] {
        if self.gps == 0 {
            return [0.0, 0.0, 0.0];
        }
        unsafe {
            let p = wb_gps_get_values(self.gps);
            if p.is_null() {
                return [0.0, 0.0, 0.0];
            }
            // 修正：wb_gps_get_values 返回的是 *const f64 (double)
            let values = std::slice::from_raw_parts(p, 3);
            [values[0] as f32, values[1] as f32, values[2] as f32]
        }
    }

    /// 获取完整的点云数据，展平为 [x1, y1, z1, x2, y2, z2, ...]
    pub fn get_lidar_points(&self) -> Vec<f32> {
        if self.lidar == 0 {
            return Vec::new();
        }

        unsafe {
            let ptr = wb_lidar_get_point_cloud(self.lidar);
            if ptr.is_null() {
                return Vec::new();
            }

            let n = wb_lidar_get_number_of_points(self.lidar) as usize;
            let points_slice: &[WbLidarPoint] = std::slice::from_raw_parts(ptr, n);

            let mut flat_data = Vec::with_capacity(n * 3);
            for p in points_slice {
                // 排除无效点和过远的点（Webots 中通常 > 1000m 为无反射）
                if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
                    flat_data.push(p.x as f32);
                    flat_data.push(p.y as f32);
                    flat_data.push(p.z as f32);
                }
            }
            flat_data
        }
    }

    // 获取速度 (m/s)
    pub fn get_speed(&self) -> f32 {
        unsafe {
            // wb_gps_get_speed 返回的是 m/s
            wb_gps_get_speed(self.gps) as f32
        }
    }

    /// 获取 OpenDrive 地图数据
    /// 在 Webots 自动驾驶场景中，地图通常存在于项目目录的特定位置
    pub fn get_opendrive(&self) -> String {
        // 这里的路径需根据你的 Webots 项目实际位置调整
        // 或者从 Webots 的自定义字段中读取内容
        std::fs::read_to_string("world_map.xodr").unwrap_or_default()
    }

    /// 获取目标路点 (Objective Waypoints)
    /// 这通常是全局路径规划器的输出，这里先返回一个预设的轨迹数据（适配 Python 格式）
    pub fn get_objective_waypoints(&self) -> Vec<[f32; 2]> {
        // 示例：返回一个简单的坐标点数组
        vec![[0.0, 0.0], [10.0, 0.0], [20.0, 5.0]]
    }

    /// 核心优化：统一设置所有动力轮的速度
    pub fn set_drive_speed(&self, speed: f64) {
        unsafe {
            for &motor in &self.drive_motors {
                wb_motor_set_velocity(motor, speed);
            }
        }
    }

    /// 控制转向
    pub fn set_steering(&self, angle: f64) {
        unsafe {
            if self.steering_wheel != 0 {
                wb_motor_set_position(self.steering_wheel, angle);
            }
        }
    }

    /// 获取车辆的姿态角 [roll, pitch, yaw] (单位：弧度)
    pub fn get_attitude(&self) -> [f32; 3] {
        if self.imu == 0 {
            return [0.0, 0.0, 0.0];
        }
        unsafe {
            let p = wb_inertial_unit_get_roll_pitch_yaw(self.imu);
            if p.is_null() {
                return [0.0, 0.0, 0.0];
            }
            let values = std::slice::from_raw_parts(p, 3);
            [values[0] as f32, values[1] as f32, values[2] as f32]
        }
    }

    /// 专门获取 Yaw (偏航角)，这是世界坐标转换的关键
    pub fn get_yaw(&self) -> f32 {
        self.get_attitude()[2]
    }

    /// 核心方法：解析 OpenDRIVE 提取路点
    /// lane_id 通常对应 .xodr 中的 <road id="...">
    pub fn get_waypoints_from_opendrive(&self, road_id: &str) -> Vec<[f32; 2]> {
        if self.opendrive_data.is_empty() {
            return Vec::new();
        }

        let mut waypoints = Vec::new();

        // 解析 XML
        if let Ok(doc) = roxmltree::Document::parse(&self.opendrive_data) {
            // 1. 找到指定的 road 节点
            let road = doc
                .descendants()
                .find(|n| n.has_tag_name("road") && n.attribute("id") == Some(road_id));

            if let Some(road_node) = road {
                // 2. 遍历 planView 下的 geometry
                for geometry in road_node
                    .descendants()
                    .filter(|n| n.has_tag_name("geometry"))
                {
                    let s: f32 = geometry
                        .attribute("s")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let x: f32 = geometry
                        .attribute("x")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let y: f32 = geometry
                        .attribute("y")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let hdg: f32 = geometry
                        .attribute("hdg")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let length: f32 = geometry
                        .attribute("length")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);

                    // 3. 简单的线性采样：每隔 2 米取一个点
                    let mut current_s = 0.0;
                    while current_s <= length {
                        let wx = x + current_s * hdg.cos();
                        let wy = y + current_s * hdg.sin();
                        waypoints.push([wx, wy]);
                        current_s += 2.0; // 采样步长
                    }
                }
            }
        }

        println!(
            "Extracted {} waypoints from Road {}",
            waypoints.len(),
            road_id
        );
        waypoints
    }
}

impl Drop for WebotsRobot {
    fn drop(&mut self) {
        unsafe { wb_robot_cleanup() };
    }
}
