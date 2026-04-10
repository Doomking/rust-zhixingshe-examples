use accelerometer::Accelerometer;
use log::*;
use icm42670::Icm42670;
use esp_idf_hal::i2c::I2cDriver;
use std::time::Duration;
use std::thread;

pub fn imu_thread(mut imu: Icm42670<I2cDriver<'static>>) {
    info!("Starting IMU thread...");

    let mut tick = 0u32;
    let mut is_upright = false;

    let mut upright_count = 0u8;
    let mut flip_count = 0u8;

    loop {
        // icm42670 0.1.0 uses Accelerometer trait
        match imu.accel_norm() {
            Ok(accel) => {
                let z = accel.z;

                if tick % 20 == 0 {
                    info!("IMU Z={:.2} | upright={}", z, is_upright);
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
                        if flip_count >= 3 {
                            info!("FLIP CONFIRMED! Z={:.2} Sending lock_screen...", z);

                            let (tx, _) = crate::get_flip_channel();
                            if let Ok(sender) = tx.lock() {
                                let _ = sender.send(true);
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
                warn!("IMU Read Failed! Error: {:?}. Retrying...", e);
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}
