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
        info!("Initializing ES7210 ADC (I2C) with ESP-ADF OFFICIAL C-SOURCE TRUTH...");

        // 1. 强制复位
        self.write_reg(0x00, 0xFF)?;
        Timer::after(Duration::from_millis(20)).await;
        self.write_reg(0x00, 0x41)?;

        // 2. 初始状态：先关闭时钟和电源，安全配置
        self.write_reg(0x01, 0x1F)?;
        self.write_reg(0x02, 0x10)?; // 官方配置：时间控制

        // 3. 官方配置：高通滤波器 (消除直流偏置)
        self.write_reg(0x22, 0x0A)?;
        self.write_reg(0x23, 0x0A)?;

        // 4. 麦克风偏置与模拟供电
        self.write_reg(0x40, 0x43)?;
        self.write_reg(0x41, 0x70)?;
        self.write_reg(0x42, 0x70)?;

        // 5. 时钟架构配置 (至关重要)
        self.write_reg(0x05, 0x20)?; // 官方配置：时钟发生器
        self.write_reg(0x06, 0x00)?; // 官方配置：MCLK 为 256Fs 时，分频必须为 0！
        self.write_reg(0x07, 0x00)?;
        self.write_reg(0x08, 0x00)?; // Slave 模式

        // 6. I2S 数据格式：16-bit I2S
        self.write_reg(0x11, 0x60)?;
        self.write_reg(0x12, 0x00)?;

        // 7. 硬件增益 (官方默认为 30dB)
        self.write_reg(0x43, 0x1B)?;
        self.write_reg(0x44, 0x1B)?;
        self.write_reg(0x45, 0x1B)?;
        self.write_reg(0x46, 0x1B)?;

        // 8. 物理通道映射：AMIC1(左) -> ADC1, AMIC3(右) -> ADC2
        self.write_reg(0x47, 0x08)?;
        self.write_reg(0x48, 0x0A)?;
        self.write_reg(0x49, 0x09)?;
        self.write_reg(0x4A, 0x0B)?;

        // 9. 唤醒并启动 ADC
        self.write_reg(0x04, 0x03)?; // 给 ADC1 和 ADC2 供电
        self.write_reg(0x01, 0x00)?; // 启动全部时钟树与电源！

        info!("ES7210 is FINALLY correctly powered and armed!");
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
    Timer::after(Duration::from_millis(3100)).await;
    info!("Starting Audio Recording task (RX ARMING)...");

    let mut dma_chunk = alloc::vec![0u8; 16384];

    let mut late_err_count: u32 = 0;
    let mut received_blocks: u32 = 0;
    let mut last_report_time = embassy_time::Instant::now();

    let mut transfer = i2s_rx
        .read_dma_circular_async(rx_buffer)
        .expect("Failed to start I2S DMA");

    info!("RX DMA Armed!");

    loop {
        match transfer.pop(&mut dma_chunk).await {
            Ok(n) if n > 0 => {
                received_blocks += 1;

                // 【修复2：32位降维打击】因为用 Data32Channel32，每次收到的是 4 字节
                // 我们每次切下 1024 字节的 32-bit 数据，提纯后刚好是 512 字节的 16-bit 原音！
                for chunk_slice in dma_chunk[..n].chunks(1024) {
                    let mut send_buf = [0u8; 512];
                    let mut out_idx = 0;

                    // 每次取 4 个字节 (一个 32-bit 样本)
                    for bytes in chunk_slice.chunks_exact(4) {
                        // 在小端序和标准 I2S 中，有效的声音数据(高位)存在 byte[2] 和 byte[3]
                        // 前两个字节全是 0，我们直接把它们当垃圾扔掉！
                        send_buf[out_idx] = bytes[2];
                        send_buf[out_idx + 1] = bytes[3];
                        out_idx += 2;
                    }

                    if out_idx > 0 {
                        // 把最纯净的 16-bit 声音发进 TCP 网络！
                        if let Err(_) = crate::AUDIO_STREAM_CHANNEL.try_send(send_buf) {
                            // 通道满时静默丢弃
                        }
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

        // --- 下方的状态打印代码保持不变 ---
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
                defmt::warn!("[AUDIO STATS] No errors, but NO DATA received!");
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
