use esp_idf_hal::cpu::Core;
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{I2sBiDir, I2sDriver};
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::esp_sr;
use log::*;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

pub struct CodecConfig {
    pub i2c: Arc<Mutex<I2cDriver<'static>>>,
}

// Wrapper to allow raw pointers to be sent and shared across threads
struct AfeInstance {
    handle: *const esp_sr::esp_afe_sr_iface_t,
    data: *mut esp_sr::esp_afe_sr_data_t,
}
unsafe impl Send for AfeInstance {}
unsafe impl Sync for AfeInstance {} // Required for Arc sharing

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

struct MonoPlayback {
    rx: mpsc::Receiver<Vec<u8>>,
    queue: VecDeque<Vec<u8>>,
    cur: Vec<u8>,
    off: usize,
}

impl MonoPlayback {
    fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            queue: VecDeque::new(),
            cur: Vec::new(),
            off: 0,
        }
    }

    fn poll_rx(&mut self) {
        while let Ok(v) = self.rx.try_recv() {
            if !v.is_empty() {
                debug!("[PLAYBACK] queued {} bytes", v.len());
                self.queue.push_back(v);
            }
        }
    }

    fn advance_buffer(&mut self) {
        if self.off >= self.cur.len() {
            self.cur = self.queue.pop_front().unwrap_or_default();
            self.off = 0;
        }
    }

    /// 输出 `frames` 个立体声帧（s16le 交错 L,R），与 `read_len_bytes` 一致。
    fn fill_stereo_s16le(&mut self, dst: &mut [u8], frames: usize) {
        // Boost queued mono PCM (wake/done); levels from `say`+ffmpeg are usually well below full scale.
        const PLAYBACK_GAIN: i32 = 3;
        self.poll_rx();
        for i in 0..frames {
            self.advance_buffer();
            let m = if self.off + 2 <= self.cur.len() {
                let raw = i16::from_le_bytes([self.cur[self.off], self.cur[self.off + 1]]) as i32;
                self.off += 2;
                (raw * PLAYBACK_GAIN).clamp(i16::MIN as i32, i16::MAX as i32) as i16
            } else {
                0i16
            };
            let b = m.to_le_bytes();
            let idx = i * 4;
            dst[idx..idx + 2].copy_from_slice(&b);
            dst[idx + 2..idx + 4].copy_from_slice(&b);
        }
    }
}

pub fn audio_thread(
    i2s_driver: I2sDriver<'static, I2sBiDir>,
    config: CodecConfig,
    playback_rx: mpsc::Receiver<Vec<u8>>,
) {
    info!("Audio Engine: Using 3-thread decoupled pipeline with library state.");

    let i2s_driver = Box::leak(Box::new(i2s_driver));
    let (mut i2s_rx, mut i2s_tx) = i2s_driver.split();

    // I2S warm-up: Stereo 16-bit (2 bytes/sample × 2 channels × 512 frames).
    const WARMUP_FRAMES: usize = 512;
    let mut warmup_buf = vec![0u8; WARMUP_FRAMES * 2 * 2];
    let warmup_silence = vec![0u8; WARMUP_FRAMES * 2 * 2];
    const WARMUP_ITERATIONS: u32 = 24;
    info!("[AUDIO] I2S warm-up ({} iters, Stereo 16-bit) before codec I2C init", WARMUP_ITERATIONS);
    for _ in 0..WARMUP_ITERATIONS {
        let _ = i2s_tx.write(&warmup_silence, 10);
        let _ = i2s_rx.read(&mut warmup_buf, 1000);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    if let Ok(mut i2c) = config.i2c.lock() {
        if let Err(e) = es7210_init(&mut *i2c) {
            error!("🚨 ES7210 Init Error: {:?}", e);
        }
        if let Err(e) = es8311_init(&mut *i2c) {
            error!("🚨 ES8311 Init Error: {:?}", e);
        }
    }

    // 1. Initialize SR Models
    let model_tag = std::ffi::CString::new("model").unwrap();
    let models = unsafe { esp_sr::esp_srmodel_init(model_tag.as_ptr()) };
    if models.is_null() {
        error!("MODEL_LOADER: Failed to load models from partition 'model'. WakeNet will be disabled.");
    } else {
        info!("MODEL_LOADER: Successfully loaded srmodels.");
    }

    // 2. AFE Engine Setup — log heap before/after to detect memory issues
    info!(
        "[HEAP] Before AFE: free internal={}KB, free PSRAM={}KB",
        unsafe { esp_idf_svc::sys::esp_get_free_internal_heap_size() } / 1024,
        unsafe { esp_idf_svc::sys::esp_get_free_heap_size() } / 1024,
    );
    let afe = unsafe {
        let format_str = std::ffi::CString::new("M").unwrap();
        let afe_config_ptr = esp_sr::afe_config_init(
            format_str.as_ptr(),
            models,
            esp_sr::afe_type_t_AFE_TYPE_SR,
            esp_sr::afe_mode_t_AFE_MODE_HIGH_PERF,
        );

        if afe_config_ptr.is_null() {
            error!("AFE ERROR: Failed to init AFE config.");
            return;
        }

        let afe_config = afe_config_ptr.as_mut().unwrap();
        afe_config.wakenet_init = true;
        afe_config.vad_init = true;
        afe_config.vad_mode = esp_sr::vad_mode_t_VAD_MODE_3;
        afe_config.afe_linear_gain = 1.0;
        afe_config.aec_init = false;
        afe_config.agc_init = true;
        afe_config.memory_alloc_mode = esp_sr::afe_memory_alloc_mode_t_AFE_MEMORY_ALLOC_INTERNAL_PSRAM_BALANCE;

        let handle_ptr = esp_sr::esp_afe_handle_from_config(afe_config);
        let handle = handle_ptr.as_ref().expect("Failed to load AFE handle");
        let data = (handle.create_from_config.unwrap())(afe_config);
        if data.is_null() {
            panic!("Failed to create AFE data instance");
        }

        AfeInstance {
            handle: handle_ptr,
            data,
        }
    };

    info!(
        "[HEAP] After AFE: free internal={}KB, free PSRAM={}KB",
        unsafe { esp_idf_svc::sys::esp_get_free_internal_heap_size() } / 1024,
        unsafe { esp_idf_svc::sys::esp_get_free_heap_size() } / 1024,
    );

    // 3. Explicitly enable WakeNet, VAD, and AGC after AFE creation
    unsafe {
        let handle = afe.handle.as_ref().unwrap();

        if let Some(f) = handle.enable_wakenet {
            let ret = f(afe.data);
            info!("[AFE] enable_wakenet() returned {} (-1=fail, 0=disabled, 1=enabled)", ret);
        } else {
            warn!("[AFE] enable_wakenet not available in this ESP-SR version");
        }

        if let Some(f) = handle.set_wakenet_threshold {
            let ret = f(afe.data, 1, 0.2);
            info!("[AFE] set_wakenet_threshold(index=1, thresh=0.2) returned {} (-1=fail, 1=ok)", ret);
        }

        if let Some(f) = handle.enable_agc {
            let ret = f(afe.data);
            info!("[AFE] enable_agc() returned {} (-1=fail, 0=disabled, 1=enabled)", ret);
        } else {
            warn!("[AFE] enable_agc not available in this ESP-SR version");
        }

        if let Some(f) = handle.enable_vad {
            let ret = f(afe.data);
            info!("[AFE] enable_vad() returned {} (-1=fail, 0=disabled, 1=enabled)", ret);
        } else {
            warn!("[AFE] enable_vad not available in this ESP-SR version");
        }

        if let Some(f) = handle.print_pipeline {
            f(afe.data);
        }
    }

    // 4. Query AFE for correct chunk sizes (critical for WakeNet frame alignment)
    let feed_chunksize = unsafe {
        let handle = afe.handle.as_ref().unwrap();
        (handle.get_feed_chunksize.unwrap())(afe.data) as usize
    };
    let total_channels = unsafe {
        let handle = afe.handle.as_ref().unwrap();
        (handle.get_channel_num.unwrap())(afe.data) as usize
    };
    info!(
        "[AFE] feed_chunksize={} samples/ch, total_channels={}, format=\"M\" (1 mic)",
        feed_chunksize, total_channels
    );
    info!(
        "[AFE] sizeof(afe_fetch_result_t) = {} bytes",
        std::mem::size_of::<esp_sr::afe_fetch_result_t>()
    );

    let chunk_size = feed_chunksize;
    let (feed_tx, feed_rx) = mpsc::sync_channel::<Vec<i16>>(64);

    let afe_shared = Arc::new(afe);
    let afe_feed = afe_shared.clone();
    let afe_fetch = afe_shared.clone();

    // --- THREAD 2: AFE FEED ---
    ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"audio-feed\0").unwrap()),
        stack_size: 15 * 1024,
        priority: 15,
        pin_to_core: Some(Core::Core1),
        ..Default::default()
    }
    .set()
    .ok();

    thread::spawn(move || {
        let handle = unsafe { afe_feed.handle.as_ref().unwrap() };
        let mut feed_count: u64 = 0;
        while let Ok(samples) = feed_rx.recv() {
            feed_count += 1;

            if feed_count <= 3 || feed_count % 500 == 0 {
                let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                let min_val = samples.iter().copied().min().unwrap_or(0);
                let max_val = samples.iter().copied().max().unwrap_or(0);
                let sum: i64 = samples.iter().map(|&s| s as i64).sum();
                let dc_offset = sum / samples.len() as i64;
                info!(
                    "[AFE-FEED] #{}: {} samples, peak={}, min={}, max={}, dc_offset={}, first8=[{},{},{},{},{},{},{},{}]",
                    feed_count, samples.len(), peak, min_val, max_val, dc_offset,
                    samples[0], samples[1], samples[2], samples[3],
                    samples[4], samples[5], samples[6], samples[7],
                );
            }

            let feed_ret = unsafe {
                (handle.feed.unwrap())(afe_feed.data, samples.as_ptr() as *const i16)
            };
            if feed_count <= 3 {
                info!("[AFE-FEED] #{}: feed() returned {}", feed_count, feed_ret);
            } else if feed_ret <= 0 && feed_count % 500 == 0 {
                warn!("[AFE-FEED] #{}: feed() returned {} (expected >0)", feed_count, feed_ret);
            }
        }
        warn!("[AFE-FEED] Channel closed, feed thread exiting.");
    });

    // --- THREAD 3: AFE FETCH ---
    ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"audio-fetch\0").unwrap()),
        stack_size: 15 * 1024,
        priority: 15,
        pin_to_core: Some(Core::Core1),
        ..Default::default()
    }
    .set()
    .ok();

    thread::spawn(move || {
        let handle = unsafe { afe_fetch.handle.as_ref().unwrap() };
        let audio_ch = crate::get_audio_channel();
        let voice_ch = crate::get_voice_channel();
        let mut speech_active = false;
        let mut wakeup_triggered = false;
        let mut fetch_count: u64 = 0;
        let mut last_vad_log: u64 = 0;

        loop {
            unsafe {
                let res_ptr = (handle.fetch.unwrap())(afe_fetch.data);
                if res_ptr.is_null() {
                    continue;
                }
                let res = *res_ptr;
                fetch_count += 1;

                // One-time raw memory dump to verify struct layout
                if fetch_count == 1 {
                    let raw = std::slice::from_raw_parts(
                        res_ptr as *const u8,
                        std::mem::size_of::<esp_sr::afe_fetch_result_t>(),
                    );
                    let hex: Vec<String> = raw.iter().map(|b| format!("{:02x}", b)).collect();
                    info!("[AFE-FETCH] RAW STRUCT DUMP ({} bytes):", raw.len());
                    for (i, chunk) in hex.chunks(16).enumerate() {
                        info!("  +{:03}: {}", i * 16, chunk.join(" "));
                    }
                }

                // Periodic diagnostics (every ~500 fetches ≈ 16s at 32ms/frame)
                if fetch_count <= 3 || fetch_count % 500 == 0 {
                    // Check output data peak to verify AFE is producing real audio
                    let out_peak = if !res.data.is_null() && res.data_size > 0 {
                        let out_slice = std::slice::from_raw_parts(
                            res.data, (res.data_size as usize) / 2,
                        );
                        out_slice.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0)
                    } else {
                        0
                    };
                    info!(
                        "[AFE-FETCH] #{}: ret={}, vad={}, wakeup_state={}, wake_idx={}, vol={:.1}dB, data_size={}, out_peak={}, ringbuf_free={:.0}%",
                        fetch_count,
                        res.ret_value,
                        res.vad_state,
                        res.wakeup_state,
                        res.wake_word_index,
                        res.data_volume,
                        res.data_size,
                        out_peak,
                        res.ringbuff_free_pct * 100.0,
                    );
                }

                // Log VAD state transitions
                let vad_now = res.vad_state as u64;
                if vad_now != last_vad_log {
                    info!(
                        "[AFE-FETCH] VAD transition: {} -> {} (fetch #{})",
                        last_vad_log, vad_now, fetch_count
                    );
                    last_vad_log = vad_now;
                }

                if res.wakeup_state != 0 && !wakeup_triggered {
                    info!("[AUDIO] 🟢 WAKEUP DETECTED: state={}, ID={}, fetch #{}", res.wakeup_state, res.wake_word_index, fetch_count);
                    wakeup_triggered = true;
                    if let Ok(mut status) = crate::get_status().write() {
                        status.voice_state = 1;
                        status.last_activity = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                    }
                    if let Ok(tx) = voice_ch.0.lock() {
                        let _ = tx.send(crate::protocol::MSG_VOICE_START as u32);
                    }
                    crate::enqueue_playback_pcm(crate::voice_prompts::WAKE_ACK_PCM.to_vec());
                }

                if res.ret_value == 0 {
                    let is_speech = res.vad_state == esp_sr::vad_state_t_VAD_SPEECH;
                    if is_speech && wakeup_triggered {
                        if !speech_active {
                            speech_active = true;
                            info!("[AUDIO] SPEECH START");
                        }
                        if !res.data.is_null() && res.data_size > 0 {
                            let slice = std::slice::from_raw_parts(
                                res.data as *const u8,
                                res.data_size as usize,
                            );
                            if let Ok(tx) = audio_ch.0.lock() {
                                let _ = tx.send(slice.to_vec());
                            }
                        }
                    } else if speech_active && !is_speech {
                        speech_active = false;
                        wakeup_triggered = false;
                        info!("[AUDIO] SPEECH END");
                        if let Ok(mut status) = crate::get_status().write() {
                            status.voice_state = 0;
                        }
                        if let Ok(tx) = voice_ch.0.lock() {
                            let _ = tx.send(crate::protocol::MSG_VOICE_END as u32);
                        }
                    }
                }
            }
        }
    });

    // --- Main I2S capture: Stereo read → extract L channel → DC removal → AFE feed ---
    let stereo_samples = chunk_size * 2;
    let mut stereo_buf = vec![0i16; stereo_samples];
    let read_len_bytes = stereo_samples * std::mem::size_of::<i16>();
    let mut tx_stereo_buf = vec![0u8; read_len_bytes];
    let mut mono_for_afe = vec![0i16; chunk_size];
    let mut log_count: u32 = 0;
    let mut drop_count: u64 = 0;
    let mut playback = MonoPlayback::new(playback_rx);

    info!(
        "[I2S] Stereo 16-bit read → L-ch extract → DC removal → AFE: chunk_size={}, stereo_bytes={}",
        chunk_size, read_len_bytes
    );

    loop {
        playback.fill_stereo_s16le(&mut tx_stereo_buf, chunk_size);
        if let Err(e) = i2s_tx.write(&tx_stereo_buf, 50) {
            warn!("[PLAYBACK] i2s_tx.write failed: {:?}", e);
        }

        let read_buf_u8 = unsafe {
            std::slice::from_raw_parts_mut(
                stereo_buf.as_mut_ptr() as *mut u8,
                read_len_bytes,
            )
        };

        if let Ok(n) = i2s_rx.read(read_buf_u8, 50) {
            if n == read_len_bytes {
                log_count += 1;

                // Raw stereo diagnostic (before any processing)
                if log_count <= 5 || log_count % 500 == 0 {
                    let raw_nz = stereo_buf.iter().filter(|&&v| v != 0).count();
                    // Collect first 10 non-zero positions to reveal the spacing pattern
                    let mut nz_positions = Vec::with_capacity(10);
                    for (idx, &v) in stereo_buf.iter().enumerate() {
                        if v != 0 {
                            nz_positions.push(idx);
                            if nz_positions.len() >= 10 { break; }
                        }
                    }
                    info!(
                        "[RAW-STEREO] #{}: {} i16, non_zero={}/{}, first16=[{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}], nz_pos={:?}",
                        log_count, stereo_samples, raw_nz, stereo_samples,
                        stereo_buf[0], stereo_buf[1], stereo_buf[2], stereo_buf[3],
                        stereo_buf[4], stereo_buf[5], stereo_buf[6], stereo_buf[7],
                        stereo_buf[8], stereo_buf[9], stereo_buf[10], stereo_buf[11],
                        stereo_buf[12], stereo_buf[13], stereo_buf[14], stereo_buf[15],
                        nz_positions,
                    );
                }

                // Extract left channel: stereo interleaved [L0, R0, L1, R1, ...] → [L0, L1, ...]
                for i in 0..chunk_size {
                    mono_for_afe[i] = stereo_buf[i * 2];
                }

                // DC removal on mono
                let dc: i32 = mono_for_afe.iter().map(|&s| s as i32).sum::<i32>() / chunk_size as i32;
                if dc != 0 {
                    for s in mono_for_afe.iter_mut() {
                        *s = (*s as i32 - dc).clamp(-32768, 32767) as i16;
                    }
                }

                if log_count <= 5 || log_count % 500 == 0 {
                    let peak = mono_for_afe.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                    let min_val = mono_for_afe.iter().copied().min().unwrap_or(0);
                    let max_val = mono_for_afe.iter().copied().max().unwrap_or(0);
                    let nz = mono_for_afe.iter().filter(|&&v| v != 0).count();
                    info!(
                        "[I2S→AFE] #{}: dc={}, peak={}, min={}, max={}, non_zero={}/{}, first8=[{},{},{},{},{},{},{},{}]",
                        log_count, dc, peak, min_val, max_val, nz, chunk_size,
                        mono_for_afe[0], mono_for_afe[1], mono_for_afe[2], mono_for_afe[3],
                        mono_for_afe[4], mono_for_afe[5], mono_for_afe[6], mono_for_afe[7],
                    );
                }

                if let Err(_) = feed_tx.try_send(mono_for_afe.clone()) {
                    drop_count += 1;
                    if drop_count <= 5 || drop_count % 100 == 0 {
                        warn!("[I2S] AFE feed channel full — frame dropped (total: {})", drop_count);
                    }
                }
            } else {
                warn!("I2S READ MISMATCH: got {}, expected {}", n, read_len_bytes);
            }
        }
    }
}
