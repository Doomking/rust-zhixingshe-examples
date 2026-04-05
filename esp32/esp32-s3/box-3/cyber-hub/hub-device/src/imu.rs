use accelerometer::Accelerometer;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use icm42670::Icm42670;
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;

#[embassy_executor::task]
pub async fn imu_task(mut imu: Icm42670<CriticalSectionDevice<'static, I2c<'static, Blocking>>>) {
    info!("Starting IMU task...");

    let mut tick = 0u32;
    let mut is_upright = false;

    let mut upright_count = 0u8;
    let mut flip_count = 0u8;

    loop {
        match imu.accel_norm() {
            Ok(accel) => {
                let z = accel.z;

                if tick % 10 == 0 {
                    info!("IMU Z={} | upright={}", z, is_upright);
                }
                tick += 1;

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
                else if z > 0.3 {
                    if is_upright {
                        flip_count += 1;
                        info!("Flip detecting... count: {}/3 (Z={})", flip_count, z);

                        if flip_count >= 3 {
                            info!("FLIP CONFIRMED! Z={} Sending lock_screen...", z);

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
                    upright_count = 0;
                    flip_count = 0;
                }
            }
            Err(e) => {
                error!("IMU Read Failed! Error: {:?}. Retrying once...", defmt::Debug2Format(&e));
                // 立即重试一次
                match imu.accel_norm() {
                    Ok(accel) => {
                        let z = accel.z;
                        if tick % 10 == 0 {
                            info!("IMU Z={} | upright={}", z, is_upright);
                        }
                        tick += 1;
                        // 这里可以继续处理 z，为保持代码简洁直接等下次循环
                    }
                    Err(_) => {
                        error!("IMU Retry Failed! Cooling down bus for 2s...");
                        Timer::after(Duration::from_secs(2)).await;
                    }
                }
            }
        }

        Timer::after(Duration::from_millis(50)).await;
    }
}
