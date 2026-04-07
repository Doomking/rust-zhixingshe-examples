use defmt::*;
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c;
use esp_hal::i2s::master::{I2sRx, I2sTx};

pub const ES7210_ADDR: u8 = 0x40;
pub const ES8311_ADDR: u8 = 0x18;

pub struct Es7210<I2C> {
    i2c: I2C,
}

impl<I2C: I2c> Es7210<I2C> {
    pub fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), ()> {
        self.i2c.write(ES7210_ADDR, &[reg, val]).map_err(|_| ())
    }

    pub async fn init(&mut self) -> Result<(), ()> {
        info!("ES7210 16-BIT SLAVE INIT: Final alignment...");
        self.write_reg(0x00, 0xFF)?;
        Timer::after_millis(10).await;
        self.write_reg(0x00, 0x41)?;
        self.write_reg(0x40, 0x43)?;
        self.write_reg(0x02, 0xC1)?;
        self.write_reg(0x03, 0x00)?;
        self.write_reg(0x07, 0x20)?;
        self.write_reg(0x04, 0x01)?;
        self.write_reg(0x05, 0x00)?;
        self.write_reg(0x08, 0x10)?;
        self.write_reg(0x11, 0x70)?; // Set to 16-bit width (from 0x60/18-bit)
        self.write_reg(0x0E, 0x00)?;
        self.write_reg(0x12, 0x0F)?;
        self.write_reg(0x13, 0x00)?;
        self.write_reg(0x4B, 0x00)?;
        self.write_reg(0x4C, 0x00)?;
        self.write_reg(0x43, 0xBF)?; // ADC1 Volume (Set to 0dB baseline)
        self.write_reg(0x44, 0xBF)?; // ADC2 Volume
        self.write_reg(0x45, 0xBF)?; // ADC3 Volume
        self.write_reg(0x46, 0xBF)?; // ADC4 Volume
        self.write_reg(0x47, 0x0A)?; // MIC1 PGA Gain (30dB)
        self.write_reg(0x48, 0x0A)?; // MIC2 PGA Gain (30dB)
        self.write_reg(0x49, 0x0A)?; // MIC3 PGA Gain (30dB)
        self.write_reg(0x4A, 0x0A)?; // MIC4 PGA Gain (30dB)
        self.write_reg(0x09, 0x30)?;
        self.write_reg(0x0A, 0x30)?;
        self.write_reg(0x00, 0x01)?;
        info!("ES7210: Armed and Ready.");
        Ok(())
    }
}

pub struct Es8311<I2C> {
    i2c: I2C,
}

impl<I2C: I2c> Es8311<I2C> {
    pub fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), ()> {
        self.i2c.write(ES8311_ADDR, &[reg, val]).map_err(|_| ())
    }

    pub async fn init(&mut self) -> Result<(), ()> {
        info!("Initializing ES8311...");
        self.write_reg(0x45, 0x00)?;
        self.write_reg(0x01, 0x30)?;
        self.write_reg(0x02, 0x10)?;
        self.write_reg(0x03, 0x10)?;
        self.write_reg(0x16, 0x24)?;
        self.write_reg(0x09, 0x00)?;
        self.write_reg(0x04, 0x10)?;
        self.write_reg(0x05, 0x00)?;
        self.write_reg(0x14, 0xCF)?;
        self.write_reg(0x32, 0xBF)?;
        Ok(())
    }
}

extern crate alloc;

#[embassy_executor::task]
pub async fn audio_record_task(
    i2s_rx: I2sRx<'static, esp_hal::Async>,
    rx_buffer: &'static mut [u8; 32768],
) {
    info!("AUDIO CAPTURE ENGINE: Starting.");

    let mut send_buf = [0u8; 512];
    let mut out_idx = 0;
    static mut DMA_CHUNK: [u8; 32768] = [0u8; 32768];
    let mut last_report = embassy_time::Instant::now();
    let mut last_error_report = embassy_time::Instant::now();
    let mut received_blocks: u32 = 0;

    let mut transfer = i2s_rx
        .read_dma_circular_async(rx_buffer)
        .expect("DMA RX START FAIL");

    loop {
        let dma_chunk = unsafe { &mut DMA_CHUNK[..] };
        match transfer.pop(dma_chunk).await {
            Ok(n) if n > 0 => {
                received_blocks += 1;

                for frame in dma_chunk[..n].chunks_exact(4) {
                    send_buf[out_idx..out_idx + 4].copy_from_slice(frame);
                    out_idx += 4;
                    if out_idx >= 512 {
                        let _ = crate::AUDIO_STREAM_CHANNEL.try_send(send_buf);
                        out_idx = 0;
                    }
                }

                if last_report.elapsed() > Duration::from_secs(2) {
                    info!(
                        "[DIAG] {} blocks | SAMPLE: {:02x}{:02x}",
                        received_blocks, dma_chunk[0], dma_chunk[1]
                    );
                    received_blocks = 0;
                    last_report = embassy_time::Instant::now();
                }
            }
            Ok(_) => {
                Timer::after_millis(5).await;
            }
            Err(e) => {
                if last_error_report.elapsed() > Duration::from_secs(2) {
                    warn!("DMA LATE/OVERRUN: {:?}. DISCARDING SPIKE...", e);
                    last_error_report = embassy_time::Instant::now();
                }
                Timer::after_millis(20).await;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn dummy_tx_task(
    i2s_tx: I2sTx<'static, esp_hal::Async>,
    tx_buffer: &'static mut [u8; 4096],
) {
    info!("DUMMY TX HEARTBEAT: Active.");
    let tx_data = [0u8; 1024];
    let mut transfer = i2s_tx
        .write_dma_circular_async(tx_buffer)
        .expect("TX DMA FAIL");

    loop {
        let _ = transfer.push(&tx_data).await;
        Timer::after_millis(100).await;
    }
}
