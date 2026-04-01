use defmt::*;
use embassy_time::{Duration, Timer};
use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;

pub const ES8311_ADDR: u8 = 0x18;
pub const ES7210_ADDR: u8 = 0x40;

pub struct Es7210<'a> {
    i2c: &'a mut I2c<'static, Blocking>,
}

impl<'a> Es7210<'a> {
    pub fn new(i2c: &'a mut I2c<'static, Blocking>) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), ()> {
        self.i2c.write(ES7210_ADDR, &[reg, val]).map_err(|_| ())
    }

    pub async fn init(&mut self) -> Result<(), ()> {
        info!("Initializing ES7210 ADC (I2C)...");

        // 1. Reset
        self.write_reg(0x00, 0xFF)?;
        Timer::after(Duration::from_millis(10)).await;

        // 2. Power & Clock Setup
        self.write_reg(0x01, 0x20)?;
        self.write_reg(0x09, 0x30)?;
        self.write_reg(0x0a, 0x30)?;

        // 3. I2S Config (16-bit, 16kHz based on 12.288MHz MCLK)
        self.write_reg(0x11, 0x60)?; // 16-bit I2S Mode
        self.write_reg(0x07, 0x20)?; // OSR = 128
        self.write_reg(0x02, 0xC3)?; // Clock Div
        self.write_reg(0x04, 0x03)?; // LRCK Divider High (16kHz)
        self.write_reg(0x05, 0x00)?; // LRCK Divider Low

        // 4. Analog & MIC Config
        self.write_reg(0x40, 0xC3)?; // Analog power
        self.write_reg(0x41, 0x70)?; // MIC Bias
        self.write_reg(0x42, 0x70)?;
        self.write_reg(0x43, 0x1E)?; // MIC1 Gain (+24dB)
        self.write_reg(0x44, 0x1E)?; // MIC2 Gain (+24dB)
        self.write_reg(0x47, 0x08)?; // MIC1 Power up
        self.write_reg(0x48, 0x08)?; // MIC2 Power up
        self.write_reg(0x4b, 0x0f)?; // ADC 1/2 Power up

        // 5. Digital Kickstart
        self.write_reg(0x00, 0x71)?;
        self.write_reg(0x00, 0x41)?;

        info!("ES7210 initialized via I2C.");
        Ok(())
    }
}

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

    pub async fn init(&mut self) -> Result<(), ()> {
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

extern crate alloc; // 确保顶部能调用分配器

// ------------------------------------------------------------------------------------------------ //
// [后台任务 4]: 音频采集任务
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn audio_record_task(
    i2s_rx: esp_hal::i2s::master::I2sRx<'static, esp_hal::Async>,
    rx_buffer: &'static mut [u8; 16384],
) {
    info!("Waiting for TX clock to stabilize...");
    // RX 晚 100ms 启动，完美接住 TX 建立的时钟
    Timer::after(Duration::from_millis(3100)).await;
    info!("Starting Audio Recording task (RX ARMING)...");

    // 【终极绝杀】：分配一个和 DMA 底层环形缓冲区一模一样大的动态数组！
    // 这样无论积压了多少数据，pop() 都能一次性安全吞下，彻底根除 BufferTooSmall 死锁！
    let mut dma_chunk = alloc::vec![0u8; 16384];

    let mut late_err_count: u32 = 0;
    let mut received_blocks: u32 = 0;
    let mut last_report_time = embassy_time::Instant::now();

    let mut transfer = i2s_rx
        .read_dma_circular_async(rx_buffer)
        .expect("Failed to start I2S DMA");

    info!("RX DMA Armed!");

    loop {
        // 使用 16KB 的海量吞吐碗去接水
        match transfer.pop(&mut dma_chunk).await {
            Ok(n) if n > 0 => {
                received_blocks += 1;

                // 将一口气吞下的大块头，优雅地切成 512 字节的网络小块发送
                for chunk_slice in dma_chunk[..n].chunks(512) {
                    let mut send_buf = [0u8; 512];
                    let len = chunk_slice.len();
                    send_buf[..len].copy_from_slice(chunk_slice);

                    if let Err(_) = crate::AUDIO_STREAM_CHANNEL.try_send(send_buf) {
                        // 通道满时静默丢弃旧音频
                    }
                }
            }
            Ok(_) => {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(_e) => {
                if late_err_count == 0 {
                    defmt::error!("I2S DMA CRITICAL ERROR: {:?}", defmt::Debug2Format(&_e));
                }
                late_err_count += 1;
                Timer::after(Duration::from_millis(2)).await;
            }
        }

        if last_report_time.elapsed() > Duration::from_secs(5) {
            if late_err_count > 0 {
                defmt::warn!(
                    "[AUDIO STATS] DMA Errors: {} | Blocks received: {}",
                    late_err_count,
                    received_blocks
                );
                late_err_count = 0;
            } else if received_blocks > 0 {
                defmt::info!(
                    "[AUDIO STATS] Running perfectly. 0 errors, {} blocks received in last 5s.",
                    received_blocks
                );
            } else {
                defmt::warn!(
                    "[AUDIO STATS] No errors, but NO DATA received! Check hardware clocks."
                );
            }
            received_blocks = 0;
            last_report_time = embassy_time::Instant::now();
        }
    }
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 5]: 影子发送任务 (维持时钟心跳)
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn dummy_tx_task(
    i2s_tx: esp_hal::i2s::master::I2sTx<'static, esp_hal::Async>,
    tx_buffer: &'static mut [u8; 1024],
) {
    // TX 提前 100ms 准时在 3.0 秒启动，主导建立硬件时钟
    Timer::after(Duration::from_millis(3000)).await;
    info!("Starting Dummy TX task to drive hardware clocks...");

    let tx_data = [0u8; 1024];

    let mut transfer = i2s_tx
        .write_dma_circular_async(tx_buffer)
        .expect("Failed to start dummy TX DMA");

    loop {
        match transfer.push(&tx_data).await {
            Ok(_) => {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(_e) => {
                Timer::after(Duration::from_millis(10)).await;
            }
        }
    }
}
