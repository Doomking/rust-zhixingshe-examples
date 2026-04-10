use esp_idf_svc::hal::i2c::I2cDriver;
use icm42670::prelude::*;
use icm42670::{Address, Icm42670};
use log::*;
use std::thread;
use std::sync::{Arc, Mutex};
use crate::{get_flip_channel, get_status};

pub fn imu_thread(i2c_bus: Arc<Mutex<I2cDriver<'static>>>) {
    info!("Starting IMU thread with shared I2C bus...");

    loop {
        {
            let mut i2c = match i2c_bus.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
            };

            if let Ok(mut imu) = Icm42670::new(&mut *i2c, Address::Primary) {
                // Default mode is sufficient for gravity vector check
                if let Ok(accel) = imu.accel_norm() {
                    if accel.z > 0.8 {
                        if let Ok(status) = get_status().read() {
                            if status.voice_state == 0 {
                                info!("Screen Face Down! Triggering Flip Event.");
                                if let Ok(guard) = get_flip_channel().0.lock() {
                                    let sender: &std::sync::mpsc::Sender<bool> = &*guard;
                                    let _ = sender.send(true);
                                }
                            }
                        }
                    }
                }
            }
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
}
