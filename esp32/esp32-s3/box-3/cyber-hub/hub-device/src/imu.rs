use accelerometer::Accelerometer;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use icm42670::Icm42670;

#[embassy_executor::task]
pub async fn imu_task(mut imu: Icm42670<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>) {
    info!("Starting IMU task...");

    let mut tick = 0u32;
    let mut is_upright = false;

    let mut upright_count = 0u8;
    let mut flip_count = 0u8;

    loop {
        // [关键修复 1]：不再使用 if let 默默吞掉错误，把 I2C 的底层故障暴露出来
        match imu.accel_norm() {
            Ok(accel) => {
                let z = accel.z;

                if tick % 10 == 0 {
                    info!("IMU Z={} | upright={}", z, is_upright);
                }
                tick += 1;

                // 直立检测
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
                // 趴下检测
                else if z > 0.3 {
                    if is_upright {
                        flip_count += 1;
                        // [关键观测点]：打印翻转的进度条，看看是中途被清零了，还是顺利到达 3
                        info!("Flip detecting... count: {}/3 (Z={})", flip_count, z);

                        if flip_count >= 3 {
                            info!("FLIP CONFIRMED! Z={} Sending lock_screen...", z);

                            // [关键修复 2]：绝对不能用 let _ = 静默忽略发送结果！
                            // 我们必须知道到底是不是通道被塞满了。
                            match crate::FLIP_EVENT_CHANNEL.try_send(true) {
                                Ok(_) => info!("Lock screen event sent successfully!"),
                                Err(_) => error!("FLIP_EVENT_CHANNEL is FULL! Event dropped!"),
                            }

                            is_upright = false;
                            flip_count = 0;
                        }
                    }
                    upright_count = 0;
                } else {
                    // 中间地带，清零即时计数
                    upright_count = 0;
                    flip_count = 0;
                }
            }
            Err(_e) => {
                // 如果 I2C 读取失败，直接抛出鲜红的 Error，绝对不当哑巴
                error!("IMU Read Failed! I2C bus error.");
            }
        }

        // 如果你觉得太快容易误触，可以把这里稍微调慢，比如 Duration::from_millis(80)
        Timer::after(Duration::from_millis(50)).await;
    }
}
