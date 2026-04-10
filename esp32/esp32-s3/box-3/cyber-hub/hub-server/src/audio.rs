use anyhow::Result;
use hound::{WavWriter, WavSpec};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::time::Instant;
use tracing::info;
use crate::config::AppConfig;

pub struct AudioProcessor {
    is_speaking: bool,
    last_activity: Instant,
    rolling_buffer: VecDeque<i16>,
    current_utterance: Option<WavWriter<BufWriter<File>>>,
    utterance_count: u32,
    threshold: f64,
    session_id: String,
    storage_path: String,
    spec: WavSpec,
}

impl AudioProcessor {
    pub fn new(config: &AppConfig, session_id: String) -> Self {
        let spec = WavSpec {
            channels: 2,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        Self {
            is_speaking: false,
            last_activity: Instant::now(),
            rolling_buffer: VecDeque::with_capacity(16000 * 2 * 3), // ~3s
            current_utterance: None,
            utterance_count: 0,
            threshold: config.stt_threshold,
            session_id,
            storage_path: config.audio_storage_path.clone(),
            spec,
        }
    }

    pub fn process_data(&mut self, data: &[u8]) -> Result<Option<String>> {
        let mut samples = Vec::with_capacity(data.len() / 2);
        for chunk in data.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            samples.push(sample);
            
            if self.rolling_buffer.len() >= self.rolling_buffer.capacity() {
                self.rolling_buffer.pop_front();
            }
            self.rolling_buffer.push_back(sample);
        }

        let mut sum_sq = 0f64;
        for &sample in &samples {
            sum_sq += (sample as f64) * (sample as f64);
        }

        let mut finished_utterance = None;

        if !samples.is_empty() {
            let rms = (sum_sq / samples.len() as f64).sqrt();
            
            if rms > self.threshold {
                self.last_activity = Instant::now();
                if !self.is_speaking {
                    self.is_speaking = true;
                    self.utterance_count += 1;
                    println!("\x1b[32m[VAD] !!! Voice Detected! (Utterance #{})\x1b[0m", self.utterance_count);
                    
                    let filename = format!("utterance_{}_{}.wav", self.session_id, self.utterance_count);
                    let full_path = std::path::Path::new(&self.storage_path).join(&filename);
                    let mut writer = WavWriter::create(&full_path, self.spec)?;
                    for &old_sample in self.rolling_buffer.iter() {
                        let _ = writer.write_sample(old_sample);
                    }
                    self.current_utterance = Some(writer);
                }
            }

            if let Some(ref mut writer) = self.current_utterance {
                for &sample in &samples {
                    let _ = writer.write_sample(sample);
                }
            }

            if self.is_speaking && self.last_activity.elapsed().as_millis() > 1000 {
                self.is_speaking = false;
                if let Some(writer) = self.current_utterance.take() {
                    writer.finalize()?;
                    let filename = format!("utterance_{}_{}.wav", self.session_id, self.utterance_count);
                    let full_path = std::path::Path::new(&self.storage_path).join(&filename);
                    finished_utterance = Some(full_path.to_string_lossy().into_owned());
                    println!("\x1b[33m[VAD] --- End of Speech.\x1b[0m");
                }
            }
        }

        Ok(finished_utterance)
    }
}
