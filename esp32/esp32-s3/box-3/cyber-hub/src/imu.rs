use defmt::info;
use embassy_time::{Duration, Timer};
use accelerometer::Accelerometer;
use icm42670::Icm42670;

#[embassy_executor::task]
pub async fn imu_task(mut imu: Icm42670<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>) {
    info!("Starting IMU task...");

    let mut tick = 0u32;
    // is_upright = 设备已明确处于直立/正放状态，准备好检测下一次翻转
    let mut is_upright = false;
    
    // 简单的滤波/防抖计数
    let mut upright_count = 0u8;
    let mut flip_count = 0u8;

    loop {
        if let Ok(accel) = imu.accel_norm() {
            if tick % 10 == 0 {
                info!("IMU Z={} | upright={}", accel.z, is_upright);
            }
            tick += 1;

            let z = accel.z;

            // 直立检测 (需要连续 3 次判定才生效)
            if z < -0.2 {
                if !is_upright {
                    upright_count += 1;
                    if upright_count >= 3 {
                        info!("Device confirmed UPRIGHT. Ready to detect flip.");
                        is_upright = true;
                        upright_count = 0;
                    }
                }
                flip_count = 0;
            } 
            // 趴下检测 (需要连续 3 次判定 + 之前必须是直立状态)
            else if z > 0.3 {
                if is_upright {
                    flip_count += 1;
                    if flip_count >= 3 {
                        info!("FLIP CONFIRMED! Z={} Sending lock_screen...", z);
                        let _ = crate::FLIP_EVENT_CHANNEL.try_send(true);
                        is_upright = false; // 必须再次确认直立才能再次触发
                        flip_count = 0;
                    }
                }
                upright_count = 0;
            } else {
                // 中间地带，清零即时计数，但维持 is_upright 状态
                upright_count = 0;
                flip_count = 0;
            }
        }
        Timer::after(Duration::from_millis(50)).await; // 提高采样频率到 20Hz 以配合 3 次判定的防抖
    }
}
