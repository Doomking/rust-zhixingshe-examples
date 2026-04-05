use defmt::{info, warn, error};
use embassy_time::{Duration, Timer};
use crate::{SYSTEM_STATUS, STATUS_STATE};
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;
use embedded_hal::i2c::I2c as I2cTrait;

// AHT30 I2C 地址 (基于用户提供的规格表)
const AHT30_ADDR: u8 = 0x38;
// 指令集
const CMD_INIT: [u8; 3] = [0xBE, 0x08, 0x00];
const CMD_MEASURE: [u8; 3] = [0xAC, 0x33, 0x00];

#[embassy_executor::task]
pub async fn sensor_task(mut i2c_dev: CriticalSectionDevice<'static, I2c<'static, Blocking>>) {
    info!("[SENSOR] AHT30 异步任务启动，对齐底座硬件规范。");
    
    // 步骤 1: AHT30 初始化/校准
    let mut initialized = false;
    for i in 1..=3 {
        if let Ok(_) = i2c_dev.write(AHT30_ADDR, &CMD_INIT) {
            info!("[SENSOR] AHT30 初始化指令已发送 ({}).", i);
            Timer::after(Duration::from_millis(50)).await;
            
            // 检查校准状态 (读取 1 字节状态位)
            let mut status = [0u8; 1];
            if i2c_dev.read(AHT30_ADDR, &mut status).is_ok() {
                if (status[0] & 0x08) != 0 {
                    info!("[SENSOR] AHT30 已校准 (Status: {:02x}).", status[0]);
                    initialized = true;
                    break;
                }
            }
        }
        Timer::after(Duration::from_millis(200)).await;
    }

    if !initialized {
        error!("[SENSOR] AHT30 校准失败! 将尝试继续测量...");
    }

    loop {
        let mut success = false;
        
        for attempt in 1..=3 {
            // 步骤 2: 触发测量 AC 33 00
            if let Err(e) = i2c_dev.write(AHT30_ADDR, &CMD_MEASURE) {
                warn!("[SENSOR] 测量触发失败 ({}/3): {:?}", attempt, defmt::Debug2Format(&e));
                Timer::after(Duration::from_millis(100)).await; // 冲突回避
                continue;
            }

            // 步骤 3: 等待采样完成 (AHT30 建议 80ms)
            Timer::after(Duration::from_millis(100)).await;

            // 步骤 4: 读取 7 字节 (Status, Hum[19:12], Hum[11:4], Hum[3:0]/Temp[19:16], Temp[15:8], Temp[7:0], CRC)
            let mut data = [0u8; 7];
            match i2c_dev.read(AHT30_ADDR, &mut data) {
                Ok(_) => {
                    // 检查 Busy Bit (Bit 7)
                    if (data[0] & 0x80) != 0 {
                        warn!("[SENSOR] 传感器忙。等待重试...");
                        Timer::after(Duration::from_millis(100)).await;
                        continue;
                    }

                    // 步骤 5: 数值转换 (20-bit)
                    // 湿度 20bit: (Byte1 << 12) | (Byte2 << 4) | (Byte3 >> 4)
                    let hum_raw = ((data[1] as u32) << 12) | ((data[2] as u32) << 4) | ((data[3] as u32) >> 4);
                    // 温度 20bit: ((Byte3 & 0x0F) << 16) | (Byte4 << 8) | Byte5
                    let temp_raw = (((data[3] as u32) & 0x0F) << 16) | ((data[4] as u32) << 8) | (data[5] as u32);
                    
                    let hum = (hum_raw as f32) * 100.0 / 1048576.0;
                    let temp = (temp_raw as f32) * 200.0 / 1048576.0 - 50.0;
                    let hum = hum.clamp(0.0, 100.0);

                    info!("[SENSOR] AHT30 数据: Temp={}.{}C, Hum={}.{}%", 
                        (temp as i32), ((temp * 10.0) as i32 % 10).abs(), 
                        (hum as i32), ((hum * 10.0) as i32 % 10).abs());
                    
                    {
                        let state = STATUS_STATE.lock().await;
                        let mut status_ref = state.borrow_mut();
                        status_ref.local_temp = temp as i8;
                        status_ref.local_hum = hum as u8;
                    }
                    SYSTEM_STATUS.signal(());
                    success = true;
                    break;
                }
                Err(e) => {
                    warn!("[SENSOR] 数据读取失败 ({}/3): {:?}. 冷却 1s...", attempt, defmt::Debug2Format(&e));
                    Timer::after(Duration::from_secs(1)).await;
                }
            }
        }

        if !success {
            error!("[SENSOR] AHT30 (0x38) 持续通信异常，可能存在总线锁定或供电不足。");
            Timer::after(Duration::from_secs(5)).await;
        }

        // 采集频率：每 10 秒刷新一次
        Timer::after(Duration::from_secs(10)).await;
    }
}
