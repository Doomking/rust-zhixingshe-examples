//! Minimal audio hardware probe for ESP32-S3-BOX-3.
//! Single phase: PA GPIO46 = HIGH, play ~2s of 1 kHz stereo square (16 kHz I2S).
//! (Phase-2 LOW comparison removed for simpler re-test; re-add if needed.)

use anyhow::Result;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::i2c;
use esp_idf_svc::hal::i2s;
use log::*;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;

    info!("[PROBE] boot");

    let i2c0_config = i2c::I2cConfig::new().baudrate(100_u32.kHz().into());
    let mut i2c0_driver =
        i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c0_config)?;

    let slot_config = i2s::config::StdSlotConfig::philips_slot_default(
        i2s::config::DataBitWidth::Bits16,
        i2s::config::SlotMode::Stereo,
    )
    .slot_bit_width(i2s::config::SlotBitWidth::Bits16);
    let clk_config = i2s::config::StdClkConfig::from_sample_rate_hz(16000)
        .mclk_multiple(i2s::config::MclkMultiple::M256);
    let channel_config = i2s::config::Config::default()
        .auto_clear(true)
        .dma_buffer_count(6)
        .frames_per_buffer(512);
    let i2s_config = i2s::config::StdConfig::new(
        channel_config,
        clk_config,
        slot_config,
        i2s::config::StdGpioConfig::default(),
    );
    let mut i2s_driver = i2s::I2sDriver::new_std_bidir(
        peripherals.i2s0,
        &i2s_config,
        pins.gpio17,      // bclk
        pins.gpio16,      // din
        pins.gpio15,      // dout
        Some(pins.gpio2), // mclk
        pins.gpio45,      // ws
    )?;
    i2s_driver.tx_enable()?;
    i2s_driver.rx_enable()?;

    // Warm-up clocks before codec init.
    let i2s_driver = Box::leak(Box::new(i2s_driver));
    let (_i2s_rx, mut i2s_tx) = i2s_driver.split();
    let warmup_silence = vec![0u8; 2048];
    for _ in 0..24 {
        let _ = i2s_tx.write(&warmup_silence, 10);
        thread::sleep(Duration::from_millis(1));
    }

    cyber_hub_std::audio::es8311_init(&mut i2c0_driver)?;
    info!("[PROBE] ES8311 initialized");

    let mut pa_ctrl = gpio::PinDriver::output(pins.gpio46)?;
    let tone = make_square_stereo_1khz_16k();

    info!("[PROBE] GPIO46=HIGH, ~2s 1kHz tone");
    pa_ctrl.set_high()?;
    let loops = (2000 / 32).max(1);
    for _ in 0..loops {
        let _ = i2s_tx.write(&tone, 50);
        thread::sleep(Duration::from_millis(1));
    }

    info!("[PROBE] done, exiting in 500ms");
    thread::sleep(Duration::from_millis(500));
    Ok(())
}

fn make_square_stereo_1khz_16k() -> Vec<u8> {
    // 512 frames, stereo interleaved, s16le
    let frames = 512usize;
    let period = 16usize; // 16kHz / 1kHz
    let amp: i16 = 24_000;
    let mut out = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let s = if (i % period) < (period / 2) { amp } else { -amp };
        let b = s.to_le_bytes();
        out.extend_from_slice(&b); // L
        out.extend_from_slice(&b); // R
    }
    out
}
