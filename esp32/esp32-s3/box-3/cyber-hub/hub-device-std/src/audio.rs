use esp_idf_hal::i2c::I2cDriver;
use esp_idf_sys::esp_sr;
use log::{error, info};
use std::thread;
use std::time::Duration;

pub const ES7210_ADDR: u8 = 0x40;
pub const ES8311_ADDR: u8 = 0x18;

pub struct Es7210<'a, 'b> {
    i2c: &'b mut I2cDriver<'a>,
}

impl<'a, 'b> Es7210<'a, 'b> {
    pub fn new(i2c: &'b mut I2cDriver<'a>) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> anyhow::Result<()> {
        self.i2c.write(ES7210_ADDR, &[reg, val], 100)?;
        Ok(())
    }

    pub fn init(&mut self) -> anyhow::Result<()> {
        info!("ES7210 Init: Setting up 16-bit Slave mode...");
        self.write_reg(0x00, 0xFF)?;
        thread::sleep(Duration::from_millis(10));
        self.write_reg(0x00, 0x41)?;
        self.write_reg(0x40, 0x43)?;
        self.write_reg(0x02, 0xC1)?;
        self.write_reg(0x03, 0x00)?;
        self.write_reg(0x07, 0x20)?;
        self.write_reg(0x04, 0x01)?;
        self.write_reg(0x05, 0x00)?;
        self.write_reg(0x08, 0x10)?;
        self.write_reg(0x11, 0x70)?; // 16-bit
        self.write_reg(0x0E, 0x00)?;
        self.write_reg(0x12, 0x0F)?;
        self.write_reg(0x13, 0x00)?;
        self.write_reg(0x4B, 0x00)?;
        self.write_reg(0x4C, 0x00)?;
        self.write_reg(0x43, 0xBF)?; // ADC1 Vol
        self.write_reg(0x44, 0xBF)?; // ADC2 Vol
        self.write_reg(0x47, 0x0A)?; // Gain 30dB
        self.write_reg(0x48, 0x0A)?;
        self.write_reg(0x00, 0x01)?;
        info!("ES7210 Initialized.");
        Ok(())
    }
}

pub struct Es8311<'a, 'b> {
    i2c: &'b mut I2cDriver<'a>,
}

impl<'a, 'b> Es8311<'a, 'b> {
    pub fn new(i2c: &'b mut I2cDriver<'a>) -> Self {
        Self { i2c }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> anyhow::Result<()> {
        self.i2c.write(ES8311_ADDR, &[reg, val], 100)?;
        Ok(())
    }

    pub fn init(&mut self) -> anyhow::Result<()> {
        info!("ES8311 Init...");
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
        info!("ES8311 Initialized.");
        Ok(())
    }
}

pub fn audio_thread(mut i2s_rx: esp_idf_hal::i2s::I2sDriver<'static, esp_idf_hal::i2s::I2sRx>) {
    info!("Audio Thread Started. Initializing ESP-SR AFE...");

    unsafe {
        // --- 1. Load SR models from flash "model" partition ---
        let models = esp_sr::esp_srmodel_init(b"model\0".as_ptr() as *const _);
        if models.is_null() {
            error!("Failed to load SR models from 'model' partition!");
            error!("Make sure partitions.csv has a 'model' partition and flash is properly programmed.");
            return;
        }
        info!("SR models loaded from flash partition.");

        // Find wake word model (prefix "wn")
        let wn_name = esp_sr::esp_srmodel_filter(
            models,
            esp_sr::ESP_WN_PREFIX.as_ptr() as *const _,
            std::ptr::null(),
        );
        if wn_name.is_null() {
            warn!("No wake word model found! Wake word detection will be disabled.");
        } else {
            let name_str = std::ffi::CStr::from_ptr(wn_name).to_str().unwrap_or("?");
            info!("Wake word model found: {}", name_str);
        }

        let has_wakenet = !wn_name.is_null();

        // Channel mapping for ESP32-S3-BOX-3: ch0=mic1, ch1=mic2, ch2=ref(playback)
        let mut mic_ids: [u8; 2] = [0, 1];
        let mut ref_ids: [u8; 1] = [2];

        let mut afe_config = esp_sr::afe_config_t {
            aec_init: true,
            aec_mode: 1, // HIGH_PERF
            aec_filter_length: 128,
            se_init: true,
            ns_init: true,
            ns_model_name: std::ptr::null_mut(),
            afe_ns_mode: 1, // AFE_NS_MODE_NET
            vad_init: true,
            vad_mode: 3,
            vad_model_name: std::ptr::null_mut(),
            vad_min_speech_ms: 128,
            vad_min_noise_ms: 1000,
            vad_delay_ms: 128,
            vad_mute_playback: false,
            vad_enable_channel_trigger: false,
            wakenet_init: has_wakenet,
            wakenet_model_name: if has_wakenet { wn_name } else { std::ptr::null_mut() },
            wakenet_model_name_2: std::ptr::null_mut(),
            wakenet_mode: if has_wakenet { 1 } else { 0 }, // DET_MODE_90
            agc_init: true,
            agc_mode: 1, // AFE_AGC_MODE_WAKENET
            agc_compression_gain_db: 9,
            agc_target_level_dbfs: 3,
            pcm_config: esp_sr::afe_pcm_config_t {
                total_ch_num: 3, // 2 mic + 1 ref
                mic_num: 2,
                mic_ids: mic_ids.as_mut_ptr(),
                ref_num: 1,
                ref_ids: ref_ids.as_mut_ptr(),
                sample_rate: 16000,
            },
            afe_mode: 1, // SR_MODE_HIGH_PERF
            afe_type: 0, // SR_TYPE_ESP_SR
            afe_perferred_core: 1,
            afe_perferred_priority: 5,
            afe_ringbuf_size: 50,
            memory_alloc_mode: 3, // MORE_PSRAM
            afe_linear_gain: 1.0,
            debug_init: false,
            fixed_first_channel: false,
            fixed_output_channel: false,
            output_playback_channel: false,
        };

        let afe_handle_ptr = esp_sr::esp_afe_handle_from_config(&afe_config);
        if afe_handle_ptr.is_null() {
            error!("Failed to create AFE handle!");
            return;
        }
        let afe_handle = &*afe_handle_ptr;

        let afe_data = afe_handle.create_from_config.unwrap()(&mut afe_config);
        if afe_data.is_null() {
            error!("Failed to create AFE instance!");
            return;
        }

        info!("AFE Instance Created.");

        let chunk_size = afe_handle.get_feed_chunksize.unwrap()(afe_data);
        info!("AFE Chunk Size: {} samples", chunk_size);

        let mut i2s_buf = vec![0i16; chunk_size as usize * 3]; // 2 mic + 1 ref
        let mut voice_active = false;
        let mut silence_frames: u32 = 0;
        let silence_threshold: u32 = 25; // ~800ms of silence ends capture

        info!("Audio loop starting. Wake word mode: {}", if has_wakenet { "ON (say 'Hi ESP')" } else { "OFF (VAD fallback)" });

        loop {
            let i2s_byte_ptr = i2s_buf.as_mut_ptr() as *mut u8;
            let i2s_byte_len = i2s_buf.len() * 2;
            let i2s_byte_slice = std::slice::from_raw_parts_mut(i2s_byte_ptr, i2s_byte_len);

            match i2s_rx.read(i2s_byte_slice, 1000) {
                Ok(n) if n == i2s_byte_len => {
                    // Feed to AFE
                    afe_handle.feed.unwrap()(afe_data, i2s_buf.as_ptr());

                    // Fetch processed result
                    let res = afe_handle.fetch.unwrap()(afe_data);
                    if !res.is_null() {
                        let result = &*res;

                        // --- Wake Word Detection (on-device, local) ---
                        if !voice_active && result.wakeup_state == 1 {
                            info!("🎤 WAKE WORD DETECTED! Starting voice capture...");
                            voice_active = true;
                            silence_frames = 0;

                            if let Ok(mut status) = crate::get_status().write() {
                                status.voice_state = 1; // LISTENING
                            }
                            let (tx_v, _) = crate::get_voice_channel();
                            if let Ok(s) = tx_v.lock() {
                                let _ = s.send(0x10);
                            }
                        }

                        // --- Active: Stream audio to server ---
                        if voice_active {
                            let out_ptr = result.data;
                            let out_samples = chunk_size as usize;
                            let out_slice: &[i16] =
                                std::slice::from_raw_parts(out_ptr, out_samples);

                            let pcm_bytes: Vec<u8> = out_slice
                                .iter()
                                .flat_map(|&s| s.to_le_bytes().to_vec())
                                .collect();

                            let (tx_audio, _) = crate::get_audio_channel();
                            if let Ok(s) = tx_audio.lock() {
                                let _ = s.send(pcm_bytes);
                            }

                            // VAD: detect end of speech
                            if result.vad_state == 1 {
                                silence_frames = 0;
                            } else {
                                silence_frames += 1;
                                if silence_frames > silence_threshold {
                                    info!("[VAD] Silence detected. Ending voice capture.");
                                    voice_active = false;
                                    if let Ok(mut status) = crate::get_status().write() {
                                        status.voice_state = 0; // IDLE
                                    }
                                    let (tx_v, _) = crate::get_voice_channel();
                                    if let Ok(s) = tx_v.lock() {
                                        let _ = s.send(0x12);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(_) => {
                    // Partial read, skip this frame
                }
                Err(_) => {
                    // I2S read error, wait briefly and retry
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }
}
