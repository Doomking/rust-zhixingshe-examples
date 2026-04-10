use crate::AudioChannel;
use defmt::{info, warn};
use esp_hal::i2s::master::{I2sRx, I2sTx};
use esp_hal::i2c::master::{I2c, Error as I2cError};
use esp_hal::Blocking;

pub struct Es8311<'a> {
    i2c: &'a mut I2c<'a, Blocking>,
}

impl<'a> Es8311<'a> {
    pub fn new(i2c: &'a mut I2c<'a, Blocking>) -> Self {
        Self { i2c }
    }

    pub fn init(&mut self) -> Result<(), I2cError> {
        init_es8311(self.i2c)
    }
}

pub fn init_es8311(i2c: &mut I2c<'_, Blocking>) -> Result<(), I2cError> {
    info!("ES8311: Initializing codec output...");
    let mut write = |reg, val| i2c.write(0x18, &[reg, val]);
    write(0x00, 0x80)?; write(0x00, 0x00)?;
    write(0x01, 0x30)?; write(0x02, 0x10)?;
    write(0x03, 0x10)?; write(0x04, 0x10)?;
    write(0x0d, 0x01)?; write(0x0e, 0x02)?;
    write(0x12, 0x00)?; write(0x13, 0x10)?;
    write(0x14, 0x10)?; write(0x15, 0x00)?;
    write(0x32, 0x00)?; write(0x33, 0x00)?;
    write(0x37, 0x08)?; write(0x44, 0x08)?;
    write(0x17, 0xbf)?; write(0x16, 0x00)?;
    write(0x45, 0x00)?;
    info!("ES8311: Ready.");
    Ok(())
}

pub struct Es7210<'a> {
    i2c: &'a mut I2c<'a, Blocking>,
}

impl<'a> Es7210<'a> {
    pub fn new(i2c: &'a mut I2c<'a, Blocking>) -> Self {
        Self { i2c }
    }

    pub fn init(&mut self) -> Result<(), I2cError> {
        init_es7210(self.i2c)
    }
}

pub fn init_es7210(i2c: &mut I2c<'_, Blocking>) -> Result<(), I2cError> {
    info!("ES7210: Initializing codec input...");
    let mut write = |reg, val| i2c.write(0x40, &[reg, val]);
    write(0x01, 0x20)?; write(0x03, 0x10)?;
    write(0x04, 0x01)?; write(0x06, 0x00)?;
    write(0x08, 0x00)?; write(0x09, 0x20)?;
    write(0x0a, 0x02)?; write(0x0b, 0x01)?;
    write(0x0e, 0x00)?; write(0x0f, 0x00)?;
    write(0x10, 0x00)?; write(0x11, 0x00)?;
    write(0x12, 0x00)?; write(0x13, 0x00)?;
    write(0x14, 0x00)?; write(0x15, 0x00)?;
    write(0x16, 0x00)?; write(0x17, 0x00)?;
    write(0x18, 0x00)?; write(0x19, 0x00)?;
    write(0x20, 0x11)?; write(0x21, 0x10)?;
    write(0x22, 0x10)?; write(0x23, 0x10)?;
    write(0x40, 0x42)?; write(0x41, 0x70)?;
    write(0x42, 0x70)?; write(0x43, 0x1b)?;
    write(0x07, 0x00)?;
    info!("ES7210: Ready.");
    Ok(())
}

#[embassy_executor::task]
pub async fn dummy_tx_task(
    i2s_tx: I2sTx<'static, esp_hal::Async>,
    tx_buffer: &'static mut [u8; 4096],
) {
    info!("DUMMY TX HEARTBEAT: Active.");
    let mut tx_transfer = i2s_tx.write_dma_circular_async(tx_buffer).unwrap();
    loop {
        let _ = tx_transfer.push(&[0u8; 512]).await;
        embassy_time::Timer::after_millis(10).await;
    }
}

#[embassy_executor::task]
pub async fn audio_record_task(
    i2s_rx: I2sRx<'static, esp_hal::Async>,
    rx_buffer: &'static mut [u8; 16384],
    audio_channel: &'static AudioChannel,
    pop_buffer: &'static mut [u8; 8192],
) {
    info!("AUDIO CAPTURE ENGINE: Active.");
    let mut i2s_transfer = match i2s_rx.read_dma_circular_async(rx_buffer) {
        Ok(t) => {
            info!("[AUDIO] Init hardware stream OK.");
            t
        },
        Err(e) => {
            warn!("[ERROR] CRITICAL: DMA INIT FAIL: {:?}.", e);
            loop { embassy_time::Timer::after_secs(30).await; }
        }
    };

    loop {
        match i2s_transfer.pop(pop_buffer).await {
            Ok(len) => {
                const PACKET_SIZE: usize = 512;
                for chunk in pop_buffer[..len].chunks_exact(PACKET_SIZE) {
                    let mut packet = [0u8; PACKET_SIZE];
                    packet.copy_from_slice(chunk);
                    audio_channel.send(packet).await;
                }
            }
            Err(e) => { warn!("[AUDIO] DMA Pop Error: {:?}", e); }
        }
    }
}
