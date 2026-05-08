//! ES8311 DAC (speaker path) on ESP32-S3-BOX-3 over I2C.
//! Register sequence aligned with Espressif `esp_codec_dev` / `cyber-hub` bring-up.

use esp_idf_svc::hal::i2c::I2cDriver;
use log::info;

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
    info!("ES8311: init (16 kHz, 16-bit I2S, MCLK 256×fs)...");

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
    es8311_write(i2c, 0x32, 0xB8)?;
    es8311_write(i2c, 0x33, 0x00)?;

    info!("ES8311: ready (DAC unmuted).");
    Ok(())
}
