use esp_idf_svc::hal::i2c::I2cDriver;
use icm42670::prelude::*;
use icm42670::{Address, Icm42670};
use std::sync::{Arc, RwLock};
use glam::Vec2;
use log::{info, warn};

/// 工业级架构设计：将所有输入源（IMU、未来可能的触控等）统一抽象为 trait。
pub trait InputSource {
    fn current_gravity(&self) -> Vec2;
}

/// 基于 ICM42670 的具体输入实现
pub struct ImuInput {
    gravity: Arc<RwLock<Vec2>>,
}

impl InputSource for ImuInput {
    fn current_gravity(&self) -> Vec2 {
        *self.gravity.read().unwrap()
    }
}

/// 启动独立的传感器采样子线程
pub fn start_imu_thread(mut i2c_bus: I2cDriver<'static>) -> ImuInput {
    let gravity = Arc::new(RwLock::new(Vec2::new(0.0, 1.0)));
    let gravity_clone = gravity.clone();

    std::thread::spawn(move || {
        info!("IMU Sampling Thread started.");
        
        // 我们独占 I2C 总线，因此可以在循环外初始化传感器对象，避免重复执行 reset 序列
        let mut imu = match Icm42670::new(&mut i2c_bus, Address::Primary) {
            Ok(i) => i,
            Err(e) => {
                warn!("Failed to initialize ICM42670: {:?}", e);
                return; // 如果芯片不存在则退出线程
            }
        };

        // 水平面内重力分量（倾斜时 ax,ay 变化大，水才会往低处流）
        let mut filtered_g = Vec2::new(0.0, 1.0);
        let alpha = 0.38;
        let mut last_log = std::time::Instant::now();
        let mut sample_count: u32 = 0;

        loop {
            if let Ok(accel) = imu.accel_norm() {
                // icm42670 的 accel_norm 返回约「g」幅值；静止时 |a|≈1，水平面用 (ax,ay) 表示倾斜
                let w = Vec2::new(-accel.x, accel.y);
                let raw_g = if w.length() > 0.12 {
                    w.normalize()
                } else {
                    Vec2::new(0.0, 1.0)
                };
                
                // EMA (Exponential Moving Average) 进行抖动平滑
                filtered_g = filtered_g * (1.0 - alpha) + raw_g * alpha;
                
                if let Ok(mut g) = gravity_clone.write() {
                    *g = filtered_g;
                }
                sample_count += 1;
                if last_log.elapsed().as_secs() >= 1 {
                    info!(
                        "IMU: hz={} | raw=({:.3},{:.3},{:.3}) | mapped=({:.3},{:.3}) | filtered=({:.3},{:.3})",
                        sample_count,
                        accel.x,
                        accel.y,
                        accel.z,
                        raw_g.x,
                        raw_g.y,
                        filtered_g.x,
                        filtered_g.y
                    );
                    sample_count = 0;
                    last_log = std::time::Instant::now();
                }
            } else {
                warn!("Failed to read accel data");
            }
            
            // 维持约 50Hz 的高频采样以保证响应延迟 < 20ms
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });

    ImuInput { gravity }
}
