use crate::get_status;
use esp_idf_hal::i2c::I2cDriver;
use log::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const AHT30_ADDR: u8 = 0x38;
const CMD_INIT: [u8; 3] = [0xBE, 0x08, 0x00];
const CMD_MEASURE: [u8; 3] = [0xAC, 0x33, 0x00];

pub fn sensor_thread(i2c_dev: Arc<Mutex<I2cDriver<'static>>>) {
    info!("[SENSOR] AHT30 thread started.");

    // Step 1: Initialization
    let mut initialized = false;
    for _i in 1..=3 {
        if i2c_dev
            .lock()
            .unwrap()
            .write(AHT30_ADDR, &CMD_INIT, 100)
            .is_ok()
        {
            thread::sleep(Duration::from_millis(50));
            let mut status = [0u8; 1];
            if i2c_dev
                .lock()
                .unwrap()
                .read(AHT30_ADDR, &mut status, 100)
                .is_ok()
            {
                if (status[0] & 0x08) != 0 {
                    info!("[SENSOR] AHT30 Calibrated (Status: {:02x}).", status[0]);
                    initialized = true;
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(200));
    }

    if !initialized {
        warn!("[SENSOR] AHT30 calibration failed. Continuing anyway...");
    }

    loop {
        let mut success = false;
        for _ in 1..=3 {
            if i2c_dev
                .lock()
                .unwrap()
                .write(AHT30_ADDR, &CMD_MEASURE, 100)
                .is_err()
            {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            thread::sleep(Duration::from_millis(100));

            let mut data = [0u8; 7];
            if i2c_dev
                .lock()
                .unwrap()
                .read(AHT30_ADDR, &mut data, 100)
                .is_ok()
            {
                if (data[0] & 0x80) != 0 {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }

                let hum_raw =
                    ((data[1] as u32) << 12) | ((data[2] as u32) << 4) | ((data[3] as u32) >> 4);
                let temp_raw =
                    (((data[3] as u32) & 0x0F) << 16) | ((data[4] as u32) << 8) | (data[5] as u32);

                let hum = (hum_raw as f32) * 100.0 / 1048576.0;
                let temp = (temp_raw as f32) * 200.0 / 1048576.0 - 50.0;

                info!("[SENSOR] AHT30 Data: Temp={:.1}C, Hum={:.1}%", temp, hum);

                {
                    if let Ok(mut status) = get_status().write() {
                        status.local_temp = temp as i8;
                        status.local_hum = hum as u8;
                    }
                }
                success = true;
                break;
            }
        }

        if !success {
            warn!("[SENSOR] Communication timeout/error.");
        }

        thread::sleep(Duration::from_secs(10));
    }
}
