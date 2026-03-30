use defmt::info;
use embassy_time::{Duration, Timer};
use accelerometer::Accelerometer;
use icm42670::Icm42670;

#[embassy_executor::task]
pub async fn imu_task(mut imu: Icm42670<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>) {
    info!("Starting IMU task...");
    
    let mut was_flipped = false;
    let mut tick = 0;
    
    // Poll the accelerometer for Z-axis flips
    loop {
        if let Ok(accel) = imu.accel_norm() {
            // 每隔 10 次循环 (1秒) 打印一次当前的 Z 轴加速度，方便调试芯片实际的安装方向
            if tick % 10 == 0 {
                info!("IMU Axes: X={}, Y={}, Z={}", accel.x, accel.y, accel.z);
            }
            tick += 1;

            let is_flipped = accel.z > 0.3;

            // 只要是从“非趴下”变成了“趴下”，就立刻触发锁屏（不用管之前是躺着还是立着）
            if is_flipped && !was_flipped {
                info!("Box Flipped! Face down detected! Triggering lock_screen...");
                let _ = crate::FLIP_EVENT_CHANNEL.try_send(true);
            }
            
            was_flipped = is_flipped;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
}
