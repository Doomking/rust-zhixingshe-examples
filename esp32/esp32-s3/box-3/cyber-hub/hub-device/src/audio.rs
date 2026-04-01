use defmt::*;
use embassy_time::{Duration, Timer};
use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;

pub const ES8311_ADDR: u8 = 0x18;

pub struct Es8311<'a> {
    i2c: &'a mut I2c<'static, Blocking>,
}

impl<'a> Es8311<'a> {
    pub fn new(i2c: &'a mut I2c<'static, Blocking>) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), ()> {
        self.i2c.write(ES8311_ADDR, &[reg, val]).map_err(|_| ())
    }

    pub fn init(&mut self) -> Result<(), ()> {
        info!("Initializing ES8311 codec (I2C)...");

        // 基础初始化序列
        self.write_reg(0x45, 0x00)?; // 停止 ADC/DAC
        self.write_reg(0x01, 0x30)?; // 启用 MCLK/SCLK
        self.write_reg(0x02, 0x10)?; // 电源初始化

        self.write_reg(0x03, 0x10)?; // 默认时钟
        self.write_reg(0x16, 0x24)?; // 16bit 数据格式
        self.write_reg(0x09, 0x00)?; // I2S 标准模式

        self.write_reg(0x04, 0x10)?; // 启动系统
        self.write_reg(0x05, 0x00)?; // ADC 启动
        self.write_reg(0x0b, 0x00)?;
        self.write_reg(0x0c, 0x00)?;

        self.write_reg(0x14, 0xCF)?; // 提高麦克风录音音量
        self.write_reg(0x32, 0xBF)?; // 扬声器音量

        info!("ES8311 initialized via I2C.");
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 4]: 音频采集任务
// 策略：使用 I2S Circular DMA 持续读取麦克风数据
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn audio_record_task(
    i2s_rx: esp_hal::i2s::master::I2sRx<'static, esp_hal::Async>,
    rx_buffer: &'static mut [u8; 16384],
) {
    info!("Waiting for Wi-Fi and system to stabilize before starting Audio...");
    // [终极修复]：延时 3 秒，避开 Wi-Fi 握手极其消耗 CPU 的“交通管制期”
    Timer::after(Duration::from_secs(3)).await;
    info!("Starting Audio Recording task...");

    // 配置分块缓冲区（512 字节，对应我们的网络数据单元）
    let mut chunk = [0u8; 512];

    // 启动循环 DMA 传输
    // 注意：在 esp-hal 1.0 中，我们不再手动管理原始 buffer，而是通过 transfer 对象 pop 数据
    let mut transfer = i2s_rx
        .read_dma_circular_async(rx_buffer)
        .expect("Failed to start I2S DMA");
    // ------------------------------------------------ //
    // [新增]：高频统计变量
    // ------------------------------------------------ //
    let mut late_err_count: u32 = 0; // 记录丢包次数
    let mut last_report_time = embassy_time::Instant::now(); // 记录上次汇报时间
    loop {
        // 使用 async pop 获取数据。当 DMA 缓冲区有足够数据时，它会返回。
        match transfer.pop(&mut chunk).await {
            Ok(n) if n > 0 => {
                // 将采集到的 PCM 数据发送到网络同步 Channel
                // crate::AUDIO_STREAM_CHANNEL.send(chunk).await;
                warn!("Audio chunk received: {}", n);
                if let Err(_) = crate::AUDIO_STREAM_CHANNEL.try_send(chunk) {
                    // 这里可以加一句 trace 日志（但不要用 info/warn，否则会刷屏）
                    // defmt::trace!("Channel full, dropping audio chunk");
                }
            }
            Ok(_) => {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(e) => {
                // warn!("I2S DMA Pop Error: {:?}", defmt::Debug2Format(&e));
                // Timer::after(Duration::from_millis(100)).await;
                late_err_count += 1;
                Timer::after(Duration::from_millis(2)).await;
            }
        }
        // ------------------------------------------------ //
        // [新增]：低频汇报逻辑 (每隔 5 秒汇报一次)
        // ------------------------------------------------ //
        if last_report_time.elapsed() > Duration::from_secs(5) {
            if late_err_count > 0 {
                // 如果有错误，集中打印一次，然后清零
                warn!(
                    "[AUDIO STATS] I2S DMA Late Errors in last 5s: {}",
                    late_err_count
                );
                late_err_count = 0;
            } else {
                // 如果极其完美，也可以打印一句心跳日志（如果不喜欢可以注释掉）
                info!("[AUDIO STATS] Running perfectly. 0 errors in last 5s.");
            }
            // 重置计时器
            last_report_time = embassy_time::Instant::now();
        }
    }
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 5]: 影子发送任务 (专门喂饱硬件 DMA，防止 TX 报错干扰 RX)
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn dummy_tx_task(
    mut i2s_tx: esp_hal::i2s::master::I2sTx<'static, esp_hal::Async>,
    tx_buffer: &'static mut [u8; 1024],
) {
    // 必须和 RX 任务一样，延时 3 秒，等 Wi-Fi 连上再启动硬件
    Timer::after(Duration::from_secs(3)).await;
    info!("Starting Dummy TX task to silence hardware alarms...");

    // 启动 TX DMA
    let mut transfer = i2s_tx
        .write_dma_circular_async(tx_buffer)
        .expect("Failed to start dummy TX DMA");

    // 塞入全 0 的静音数据包
    let chunk = [0u8; 512];

    loop {
        // 不断地 push 数据，堵住 TX 的嘴，绝不让它触发 Underflow 中断
        if let Err(_e) = transfer.push(&chunk).await {
            // 静默处理即可
        }
    }
}
