use esp_idf_hal::cpu::Core;
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{I2sBiDir, I2sDriver};
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::esp_sr;
use log::*;
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
    info!("ES7210: Initializing codec input (aligned with verified no-std hub-device)...");

    let mut write = |reg, val| i2c.write(0x40, &[reg, val], 100);

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

pub fn es8311_init(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    info!("ES8311: Initializing codec output (aligned with verified no-std hub-device)...");

    let mut write = |reg, val| i2c.write(0x18, &[reg, val], 100);

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

pub fn audio_thread(mut i2s_driver: I2sDriver<'static, I2sBiDir>, config: CodecConfig) {
    info!("Audio Engine: Using 3-thread decoupled pipeline with library state.");

    let (mut i2s_rx, mut i2s_tx) = i2s_driver.split();

    // I2S warm-up: get BCLK/WS running before codec I2C init (matches no-std hub-device order).
    const WARMUP_CHUNK: usize = 512;
    let mut warmup_buf = vec![0i16; WARMUP_CHUNK * 2];
    let warmup_silence = vec![0u8; WARMUP_CHUNK * 4];
    const WARMUP_ITERATIONS: u32 = 24;
    info!("[AUDIO] I2S warm-up ({} iters) before codec I2C init", WARMUP_ITERATIONS);
    for _ in 0..WARMUP_ITERATIONS {
        let _ = i2s_tx.write(&warmup_silence, 10);
        let wb = unsafe {
            std::slice::from_raw_parts_mut(warmup_buf.as_mut_ptr() as *mut u8, WARMUP_CHUNK * 4)
        };
        let _ = i2s_rx.read(wb, 1000);
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

        // Lower threshold for diagnostic — makes WakeNet much more sensitive
        if let Some(f) = handle.set_wakenet_threshold {
            let ret = f(afe.data, 1, 0.4);
            info!("[AFE] set_wakenet_threshold(index=1, thresh=0.4) returned {} (-1=fail, 1=ok)", ret);
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
                    warn!("[AFE-FETCH] fetch() returned null");
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

                if res.wake_word_index > 0 && !wakeup_triggered {
                    info!("[AUDIO] 🟢 WAKEUP DETECTED: ID={}, fetch #{}", res.wake_word_index, fetch_count);
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

    // Thread 1: Main I2S Capture (uses dynamic chunk_size from AFE)
    let mut i2s_raw_buffer_16 = vec![0i16; chunk_size * 2]; // stereo: L+R interleaved
    let i2s_batch_len = i2s_raw_buffer_16.len() * 2; // in bytes
    let silence_vec = vec![0i16; chunk_size * 2];
    let mut log_count: u32 = 0;
    let mut drop_count: u64 = 0;

    info!(
        "[I2S] Capture loop started: chunk_size={}, stereo_buf={}i16, batch={}B",
        chunk_size,
        i2s_raw_buffer_16.len(),
        i2s_batch_len
    );

    loop {
        let silence_u8 = unsafe {
            std::slice::from_raw_parts(silence_vec.as_ptr() as *const u8, chunk_size * 4)
        };
        let _ = i2s_tx.write(silence_u8, 10);

        let i2s_byte_slice = unsafe {
            std::slice::from_raw_parts_mut(i2s_raw_buffer_16.as_mut_ptr() as *mut u8, i2s_batch_len)
        };

        if let Ok(n) = i2s_rx.read(i2s_byte_slice, 1000) {
            if n == i2s_batch_len {
                // One-time dump of raw I2S buffer (before L/R separation)
                log_count += 1;
                if log_count == 1 {
                    let r = &i2s_raw_buffer_16;
                    info!(
                        "[I2S-RAW] first 32 i16: [{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}]",
                        r[0],r[1],r[2],r[3],r[4],r[5],r[6],r[7],
                        r[8],r[9],r[10],r[11],r[12],r[13],r[14],r[15],
                        r[16],r[17],r[18],r[19],r[20],r[21],r[22],r[23],
                        r[24],r[25],r[26],r[27],r[28],r[29],r[30],r[31],
                    );
                }

                let mut mono_buf = vec![0i16; chunk_size];
                let mut peak_l: i16 = 0;
                let mut peak_r: i16 = 0;

                for i in 0..chunk_size {
                    let l = i2s_raw_buffer_16[i * 2];
                    let r = i2s_raw_buffer_16[i * 2 + 1];
                    peak_l = peak_l.max(l.abs());
                    peak_r = peak_r.max(r.abs());
                    mono_buf[i] = l;
                }

                if log_count >= 100 {
                    info!("🎤 AUDIO PEAK - L: {}, R: {}", peak_l, peak_r);
                    log_count = 0;
                }

                if let Err(_) = feed_tx.try_send(mono_buf) {
                    drop_count += 1;
                    if drop_count <= 5 || drop_count % 100 == 0 {
                        warn!("[I2S] ⚠️ AFE feed channel full — frame dropped (total: {})", drop_count);
                    }
                }
            } else {
                warn!("⚠️ I2S READ MISMATCH: got {}, expected {}", n, i2s_batch_len);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
