//! ES7210 (mic ADC) + ES8311 (speaker DAC) I2C bring-up for BOX-3.

use esp_idf_svc::hal::i2c::I2cDriver;
use log::*;
use std::sync::{Arc, Mutex};

pub struct CodecConfig {
    pub i2c: Arc<Mutex<I2cDriver<'static>>>,
}

pub fn es7210_init(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    info!("ES7210: Initializing codec input (aligned with official espressif driver, 16-bit I2S, 256fs/16kHz)...");

    let mut write = |reg: u8, val: u8| -> anyhow::Result<()> {
        i2c.write(0x40, &[reg, val], 100)?;
        Ok(())
    };

    // 1. Software reset
    write(0x00, 0xFF)?;
    write(0x00, 0x32)?;

    // 2. Power-up initialization timing (official: 0x30 for both)
    write(0x09, 0x30)?;
    write(0x0A, 0x30)?;

    // 3. HPF configuration for ADC1-4 (official values)
    write(0x23, 0x2A)?;
    write(0x22, 0x0A)?;
    write(0x21, 0x2A)?;
    write(0x20, 0x0A)?;

    // 4. I2S format: standard I2S (0x00) + 16-bit (0x60) = 0x60
    //    CRITICAL FIX: was 0x00 (=24-bit!), now 0x60 (=16-bit)
    write(0x11, 0x60)?;
    // Non-TDM mode (standard I2S, 2ch on SDOUT1)
    write(0x12, 0x00)?;

    // 5. Analog power + VMID
    write(0x40, 0xC3)?;

    // 6. MIC1-2 bias = 2.87V, MIC3-4 bias = 2.87V
    write(0x41, 0x70)?;
    write(0x42, 0x70)?;

    // 7. MIC1-4 gain = ~30dB (0x1B | 0x10 = 0x1B per official pattern)
    write(0x43, 0x1B)?;
    write(0x44, 0x1B)?;
    write(0x45, 0x1B)?;
    write(0x46, 0x1B)?;

    // 8. Power on MIC1-4
    write(0x47, 0x08)?;
    write(0x48, 0x08)?;
    write(0x49, 0x08)?;
    write(0x4A, 0x08)?;

    // 9. Clock config for 256fs @ 16kHz (MCLK=4.096MHz)
    //    from coeff table: adc_div=1, dll=1, doubler=1, osr=0x20
    write(0x07, 0x20)?;  // OSR (was 0x00 — wrong!)
    write(0x02, 0xC1)?;  // MAINCLK: adc_div=1 | doubler<<6 | dll<<7 = 0x01|0x40|0x80
    write(0x04, 0x01)?;  // LRCK_DIVH
    write(0x05, 0x00)?;  // LRCK_DIVL

    // 10. Power down DLL (official sequence)
    write(0x06, 0x04)?;

    // 11. Power on MIC1-2 bias & ADC1-2 & PGA1-2
    write(0x4B, 0x0F)?;
    write(0x4C, 0x0F)?;

    // 12. Enable device
    write(0x00, 0x71)?;
    write(0x00, 0x41)?;

    info!("ES7210: Ready (16-bit I2S, 256fs, non-TDM).");
    Ok(())
}

/// ES8311 on ESP-BOX-3 (I2C 7-bit `0x18`). Sequence ported from Espressif
/// `esp-adf` / `esp_codec_dev` `es8311.c`: `es8311_open` → `es8311_set_fs` (16 kHz,
/// 16-bit I2S, MCLK = 256×fs = 4.096 MHz) → `es8311_start` (DAC-only) → unmute.
///
/// PA GPIO is **not** driven here (handled in `main` / probe), matching the driver's split
/// between `open` and `enable`/`es8311_pa_power`.
const ES8311_ADDR: u8 = 0x18;

#[inline]
fn es8311_write(i2c: &mut I2cDriver<'static>, reg: u8, val: u8) -> anyhow::Result<()> {
    i2c.write(ES8311_ADDR, &[reg, val], 100)
        .map_err(|e| anyhow::anyhow!("ES8311 write reg 0x{:02x}: {:?}", reg, e))
}

#[inline]
fn es8311_read(i2c: &mut I2cDriver<'static>, reg: u8) -> anyhow::Result<u8> {
    let mut b = [0u8; 1];
    i2c.write_read(ES8311_ADDR, &[reg], &mut b, 100)
        .map_err(|e| anyhow::anyhow!("ES8311 read reg 0x{:02x}: {:?}", reg, e))?;
    Ok(b[0])
}

/// Clock row from `coeff_div[]` for `mclk = 4_096_000`, `rate = 16_000` in `es8311.c`.
struct Es8311Coeff {
    pre_div: u8,
    pre_multi: u8,
    adc_div: u8,
    dac_div: u8,
    fs_mode: u8,
    lrck_h: u8,
    lrck_l: u8,
    bclk_div: u8,
    adc_osr: u8,
    dac_osr: u8,
}

const COEFF_16K_MCLK4096: Es8311Coeff = Es8311Coeff {
    pre_div: 0x01,
    pre_multi: 0x01,
    adc_div: 0x01,
    dac_div: 0x01,
    fs_mode: 0x00,
    lrck_h: 0x00,
    lrck_l: 0xff,
    bclk_div: 0x04,
    adc_osr: 0x10,
    dac_osr: 0x20,
};

fn es8311_config_sample(i2c: &mut I2cDriver<'static>, c: &Es8311Coeff) -> anyhow::Result<()> {
    let mut regv = es8311_read(i2c, 0x02)?;
    regv &= 0x07;
    regv |= (c.pre_div - 1) << 5;
    let datmp: u8 = match c.pre_multi {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => 0,
    };
    // `use_mclk == true` (I2S MCLK wired): do not apply the no-MCLK override branch.
    regv |= datmp << 3;
    es8311_write(i2c, 0x02, regv)?;

    regv = 0;
    regv |= (c.adc_div - 1) << 4;
    regv |= (c.dac_div - 1) << 0;
    es8311_write(i2c, 0x05, regv)?;

    regv = es8311_read(i2c, 0x03)?;
    regv &= 0x80;
    regv |= c.fs_mode << 6;
    regv |= c.adc_osr;
    es8311_write(i2c, 0x03, regv)?;

    regv = es8311_read(i2c, 0x04)?;
    regv &= 0x80;
    regv |= c.dac_osr;
    es8311_write(i2c, 0x04, regv)?;

    regv = es8311_read(i2c, 0x07)?;
    regv &= 0xC0;
    regv |= c.lrck_h;
    es8311_write(i2c, 0x07, regv)?;

    regv = 0;
    regv |= c.lrck_l;
    es8311_write(i2c, 0x08, regv)?;

    regv = es8311_read(i2c, 0x06)?;
    regv &= 0xE0;
    let bclk_part = if c.bclk_div < 19 {
        c.bclk_div - 1
    } else {
        c.bclk_div
    };
    regv |= bclk_part;
    es8311_write(i2c, 0x06, regv)?;
    Ok(())
}

/// `es8311_set_fs`: 16-bit I2S normal + sample-rate / MCLK coefficients.
fn es8311_set_fs_16k(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    let mut dac_iface = es8311_read(i2c, 0x09)?;
    let mut adc_iface = es8311_read(i2c, 0x0a)?;
    dac_iface |= 0x0c;
    adc_iface |= 0x0c;
    es8311_write(i2c, 0x09, dac_iface)?;
    es8311_write(i2c, 0x0a, adc_iface)?;

    dac_iface = es8311_read(i2c, 0x09)?;
    adc_iface = es8311_read(i2c, 0x0a)?;
    dac_iface &= 0xFC;
    adc_iface &= 0xFC;
    es8311_write(i2c, 0x09, dac_iface)?;
    es8311_write(i2c, 0x0a, adc_iface)?;

    es8311_config_sample(i2c, &COEFF_16K_MCLK4096)?;
    Ok(())
}

/// `es8311_start` for `ESP_CODEC_DEV_WORK_MODE_DAC`, `digital_mic == false`.
fn es8311_start_dac(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    let mut regv = 0x80u8;
    regv &= 0xBF;
    es8311_write(i2c, 0x00, regv)?;

    regv = 0x3F;
    regv &= 0x7F;
    regv &= !0x40;
    es8311_write(i2c, 0x01, regv)?;

    let mut dac_iface = es8311_read(i2c, 0x09)?;
    let mut adc_iface = es8311_read(i2c, 0x0a)?;
    dac_iface &= 0xBF;
    adc_iface &= 0xBF;
    dac_iface &= !0x40;
    es8311_write(i2c, 0x09, dac_iface)?;
    es8311_write(i2c, 0x0a, adc_iface)?;

    es8311_write(i2c, 0x17, 0xBF)?;
    es8311_write(i2c, 0x0e, 0x02)?;
    es8311_write(i2c, 0x12, 0x00)?;
    es8311_write(i2c, 0x14, 0x1a)?;

    regv = es8311_read(i2c, 0x14)?;
    regv &= !0x40;
    es8311_write(i2c, 0x14, regv)?;

    es8311_write(i2c, 0x0d, 0x01)?;
    es8311_write(i2c, 0x15, 0x40)?;
    es8311_write(i2c, 0x37, 0x08)?;
    es8311_write(i2c, 0x45, 0x00)?;
    Ok(())
}

pub fn es8311_init(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    info!("ES8311: init (esp_codec_dev es8311_open + set_fs + start, 16k/16-bit, MCLK 256×fs)...");

    let mut regv = es8311_read(i2c, 0x0d)?;
    if regv != 0xFA {
        es8311_write(i2c, 0x0d, 0xFA)?;
    }

    es8311_write(i2c, 0x44, 0x08)?;
    es8311_write(i2c, 0x44, 0x08)?;

    es8311_write(i2c, 0x01, 0x30)?;
    es8311_write(i2c, 0x02, 0x00)?;
    es8311_write(i2c, 0x03, 0x10)?;
    es8311_write(i2c, 0x16, 0x24)?;
    es8311_write(i2c, 0x04, 0x10)?;
    es8311_write(i2c, 0x05, 0x00)?;
    es8311_write(i2c, 0x0b, 0x00)?;
    es8311_write(i2c, 0x0c, 0x00)?;
    es8311_write(i2c, 0x10, 0x1F)?;
    es8311_write(i2c, 0x11, 0x7F)?;
    es8311_write(i2c, 0x00, 0x80)?;

    regv = es8311_read(i2c, 0x00)?;
    regv &= 0xBF;
    es8311_write(i2c, 0x00, regv)?;

    regv = 0x3F;
    regv &= 0x7F;
    regv &= !0x40;
    es8311_write(i2c, 0x01, regv)?;

    regv = es8311_read(i2c, 0x06)?;
    regv &= !0x20;
    es8311_write(i2c, 0x06, regv)?;

    es8311_write(i2c, 0x13, 0x10)?;
    es8311_write(i2c, 0x1b, 0x0A)?;
    es8311_write(i2c, 0x1c, 0x6A)?;
    es8311_write(i2c, 0x44, 0x58)?;

    es8311_set_fs_16k(i2c)?;
    es8311_start_dac(i2c)?;

    regv = es8311_read(i2c, 0x31)?;
    regv &= 0x9F;
    es8311_write(i2c, 0x31, regv)?;
    // 0x32: DAC digital volume. Probe tone is hot; `say`/ffmpeg prompts are much lower RMS — keep high.
    es8311_write(i2c, 0x32, 0xB8)?;
    es8311_write(i2c, 0x33, 0x00)?;

    info!("ES8311: ready (DAC path, unmuted, digital vol tuned for embedded voice PCM).");
    Ok(())
}
