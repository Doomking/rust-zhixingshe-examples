use esp_idf_svc::hal::i2s::{I2sDriver, I2sBiDir};
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::sys::esp_sr;
use log::*;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use crate::get_status;

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
    info!("ES7210 Init: Applying hardware-aligned magic sequence (30dB Gain)...");
    let seq = [
        (0x00, 0xFF), (0x00, 0x41), (0x40, 0x43), (0x02, 0xC1),
        (0x03, 0x00), (0x07, 0x20), (0x04, 0x01), (0x05, 0x00),
        (0x08, 0x10), (0x11, 0x70), (0x0E, 0x00), (0x12, 0x0F),
        (0x13, 0x00), (0x4B, 0x00), (0x4C, 0x00), (0x43, 0xBF),
        (0x44, 0xBF), (0x45, 0xBF), (0x46, 0xBF), (0x47, 0x0A),
        (0x48, 0x0A), (0x49, 0x0A), (0x4A, 0x0A), (0x09, 0x30),
        (0x0A, 0x30), (0x00, 0x01),
    ];
    for (reg, val) in seq {
        i2c.write(0x40, &[reg, val], 100)?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    info!("ES7210: Armed and Ready.");
    Ok(())
}

pub fn es8311_init(i2c: &mut I2cDriver<'static>) -> anyhow::Result<()> {
    info!("ES8311 Init: Configuring DAC and Speaker path...");
    let seq = [
        (0x45, 0x00), (0x01, 0x30), (0x02, 0x10), (0x03, 0x10),
        (0x16, 0x24), (0x09, 0x00), (0x04, 0x10), (0x05, 0x00),
        (0x14, 0xCF), (0x32, 0xBF),
    ];
    for (reg, val) in seq {
        i2c.write(0x18, &[reg, val], 100)?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    info!("ES8311: Active.");
    Ok(())
}

pub fn audio_thread(mut i2s_driver: I2sDriver<'static, I2sBiDir>, config: CodecConfig) {
    info!("Audio Engine: Using 3-thread decoupled pipeline with library state.");

    if let Ok(mut i2c) = config.i2c.lock() {
        let _ = es7210_init(&mut *i2c);
        let _ = es8311_init(&mut *i2c);
    }

    let chunk_size = 512;
    let (feed_tx, feed_rx) = mpsc::sync_channel::<Vec<i16>>(32);
    let (mut i2s_rx, mut i2s_tx) = i2s_driver.split();

    // AFE Engine Setup
    let afe = unsafe {
        let format_str = std::ffi::CString::new("MM").unwrap();
        let afe_config_ptr = esp_sr::afe_config_init(
            format_str.as_ptr(),
            std::ptr::null_mut(),
            esp_sr::afe_type_t_AFE_TYPE_SR,
            esp_sr::afe_mode_t_AFE_MODE_HIGH_PERF,
        );
        let afe_config = afe_config_ptr.as_mut().expect("Failed to init AFE config");
        afe_config.wakenet_init = true;
        afe_config.vad_init = true;
        afe_config.aec_init = false;
        afe_config.agc_init = true;
        afe_config.memory_alloc_mode = esp_sr::afe_memory_alloc_mode_t_AFE_MEMORY_ALLOC_MORE_PSRAM;
        
        let handle_ptr = esp_sr::esp_afe_handle_from_config(afe_config);
        let handle = handle_ptr.as_ref().expect("Failed to load AFE handle");
        let data = (handle.create_from_config.unwrap())(afe_config);
        if data.is_null() { panic!("Failed to create AFE data instance"); }
        
        AfeInstance { handle: handle_ptr, data }
    };

    let afe_shared = Arc::new(afe);
    let afe_feed = afe_shared.clone();
    let afe_fetch = afe_shared.clone();

    // Thread 2: AFE Feed Client
    thread::spawn(move || {
        let handle = unsafe { afe_feed.handle.as_ref().unwrap() };
        while let Ok(samples) = feed_rx.recv() {
            unsafe {
                (handle.feed.unwrap())(afe_feed.data, samples.as_ptr() as *const i16);
            }
        }
    });

    // Thread 3: AFE Fetch Client
    thread::spawn(move || {
        let handle = unsafe { afe_fetch.handle.as_ref().unwrap() };
        let audio_ch = crate::get_audio_channel();
        let voice_ch = crate::get_voice_channel();
        let mut speech_active = false;

        loop {
            unsafe {
                let res_ptr = (handle.fetch.unwrap())(afe_fetch.data);
                if !res_ptr.is_null() {
                    let res = *res_ptr;
                    if res.ret_value == 0 {
                        let is_speech = res.vad_state == esp_sr::vad_state_t_VAD_SPEECH;
                        if is_speech {
                            if !speech_active {
                                speech_active = true;
                                info!("[AUDIO] VAD START");
                                if let Ok(mut status) = crate::get_status().write() { status.voice_state = 1; }
                                if let Ok(tx) = voice_ch.0.lock() { 
                                    let _ = tx.send(0x10); // msg_type: Wakeup Start
                                }
                            }
                            
                            if !res.data.is_null() && res.data_size > 0 {
                                let slice = std::slice::from_raw_parts(res.data as *const u8, res.data_size as usize);
                                if let Ok(tx) = audio_ch.0.lock() { 
                                    let _ = tx.send(slice.to_vec()); // msg_type: Audio Chunk (0x11)
                                }
                            }
                        } else if speech_active {
                            speech_active = false;
                            info!("[AUDIO] VAD END");
                            if let Ok(mut status) = crate::get_status().write() { status.voice_state = 0; }
                            if let Ok(tx) = voice_ch.0.lock() { 
                                let _ = tx.send(0x12); // msg_type: Voice End
                            }
                        }
                    }
                }
            }
        }
    });

    // Thread 1: Main I2S Capture
    let mut i2s_raw_buffer = vec![0i16; chunk_size * 2];
    let i2s_byte_len = i2s_raw_buffer.len() * 2;
    let silence_vec = vec![0i16; chunk_size * 2];
    
    let mut log_count = 0;
    loop {
        let silence_u8 = unsafe { std::slice::from_raw_parts(silence_vec.as_ptr() as *const u8, i2s_byte_len) };
        let _ = i2s_tx.write(silence_u8, 10);

        let i2s_byte_slice = unsafe { std::slice::from_raw_parts_mut(i2s_raw_buffer.as_mut_ptr() as *mut u8, i2s_byte_len) };
        if let Ok(n) = i2s_rx.read(i2s_byte_slice, 1000) {
            if n == i2s_byte_len {
                let peak = i2s_raw_buffer.iter().map(|&s| s.abs()).max().unwrap_or(0);
                log_count += 1;
                if log_count >= 50 {
                    // info!("🎤 AUDIO CHECK - PEAK: {}", peak);
                    log_count = 0;
                }
                let _ = feed_tx.try_send(i2s_raw_buffer.clone());
            }
        }
    }
}
