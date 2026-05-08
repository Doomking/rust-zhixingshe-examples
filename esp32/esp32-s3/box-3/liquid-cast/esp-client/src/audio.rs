//! I2S duplex playback: mono s16le 16 kHz → stereo ES8311 line, with RX drained each period.

use crossbeam_channel::Receiver;
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{self, I2sBiDir, I2sDriver};
use log::{error, info, warn};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::codec_es8311::es8311_init;
use crate::network::AudioPacket;

/// Software gain on mono samples before duplication to L/R.
const PLAYBACK_GAIN: i32 = 3;

/// Philips stereo frames per `write`/`read` (matches `cyber-hub` I2S cadence).
const FRAMES_PER_PERIOD: usize = 512;
const AUDIO_SAMPLE_RATE: u32 = 16_000;
const MS_PER_SAMPLE_FP: u32 = 1_000_000 / AUDIO_SAMPLE_RATE; // micro-ms fixed point

static CURRENT_AUDIO_TIME_MS: AtomicU32 = AtomicU32::new(0);

pub fn current_audio_time_ms() -> u32 {
    CURRENT_AUDIO_TIME_MS.load(Ordering::Relaxed)
}

struct PcmQueue {
    rx: Receiver<AudioPacket>,
    queue: VecDeque<AudioPacket>,
    cur: AudioPacket,
    off: usize,
    cur_samples_played: u32,
    have_clock_base: bool,
}

impl PcmQueue {
    fn new(rx: Receiver<AudioPacket>) -> Self {
        Self {
            rx,
            queue: VecDeque::new(),
            cur: AudioPacket {
                timestamp_ms: 0,
                payload: Vec::new(),
            },
            off: 0,
            cur_samples_played: 0,
            have_clock_base: false,
        }
    }

    fn poll_rx(&mut self) {
        while let Ok(v) = self.rx.try_recv() {
            if !v.payload.is_empty() {
                self.queue.push_back(v);
            }
        }
    }

    fn advance_buffer(&mut self) {
        if self.off >= self.cur.payload.len() {
            if let Some(next) = self.queue.pop_front() {
                self.cur = next;
                self.have_clock_base = true;
            } else {
                self.cur.payload.clear();
                self.have_clock_base = false;
            }
            self.off = 0;
            self.cur_samples_played = 0;
        }
    }

    /// Fill `dst` with `frames` interleaved stereo s16le samples (L, R), R = L.
    fn fill_stereo_s16le(&mut self, dst: &mut [u8], frames: usize) {
        self.poll_rx();
        for i in 0..frames {
            self.advance_buffer();
            let m = if self.off + 2 <= self.cur.payload.len() {
                let raw =
                    i16::from_le_bytes([self.cur.payload[self.off], self.cur.payload[self.off + 1]])
                        as i32;
                self.off += 2;
                self.cur_samples_played = self.cur_samples_played.saturating_add(1);
                (raw * PLAYBACK_GAIN).clamp(i16::MIN as i32, i16::MAX as i32) as i16
            } else {
                0i16
            };
            let b = m.to_le_bytes();
            let idx = i * 4;
            dst[idx..idx + 2].copy_from_slice(&b);
            dst[idx + 2..idx + 4].copy_from_slice(&b);
        }

        if self.have_clock_base {
            let delta_ms = ((self.cur_samples_played as u64) * (MS_PER_SAMPLE_FP as u64) / 1000) as u32;
            CURRENT_AUDIO_TIME_MS.store(self.cur.timestamp_ms.saturating_add(delta_ms), Ordering::Relaxed);
        }
    }
}

pub fn run_playback_thread(
    i2s: I2sDriver<'static, I2sBiDir>,
    i2c: Arc<Mutex<I2cDriver<'static>>>,
    audio_rx: Receiver<AudioPacket>,
) {
    info!("Audio: I2S playback thread starting...");

    let i2s = Box::leak(Box::new(i2s));
    let (mut i2s_rx, mut i2s_tx) = i2s.split();

    let stereo_bytes = FRAMES_PER_PERIOD * 2 * 2;
    let mut tx_buf = vec![0u8; stereo_bytes];
    let mut rx_buf = vec![0u8; stereo_bytes];
    let warmup_silence = vec![0u8; stereo_bytes];

    const WARMUP_ITERS: u32 = 24;
    info!("Audio: I2S warm-up ({} iters)...", WARMUP_ITERS);
    for _ in 0..WARMUP_ITERS {
        let _ = i2s_tx.write(&warmup_silence, 10);
        let _ = i2s_rx.read(&mut rx_buf, 1000);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    if let Ok(mut bus) = i2c.lock() {
        if let Err(e) = es8311_init(&mut *bus) {
            error!("ES8311 init failed: {:?}", e);
        }
    } else {
        error!("I2C mutex poisoned; ES8311 not configured");
    }

    let mut q = PcmQueue::new(audio_rx);

    // Pre-buffer: wait for enough audio packets before starting playback to avoid
    // initial stutter caused by the I2S loop draining faster than TCP can fill.
    const PREBUF_TARGET: usize = 8;
    const PREBUF_TIMEOUT_MS: u64 = 10_000;
    info!("Audio: pre-buffering ({} packets)...", PREBUF_TARGET);
    let prebuf_start = std::time::Instant::now();
    loop {
        q.poll_rx();
        if q.queue.len() >= PREBUF_TARGET {
            break;
        }
        if prebuf_start.elapsed().as_millis() as u64 > PREBUF_TIMEOUT_MS {
            warn!("Audio: pre-buffer timeout after {}ms with {} packets, starting anyway",
                  PREBUF_TIMEOUT_MS, q.queue.len());
            break;
        }
        // Keep I2S fed with silence during pre-buffer to avoid DMA underrun
        let _ = i2s_tx.write(&warmup_silence, 10);
        let _ = i2s_rx.read(&mut rx_buf, 10);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    info!(
        "Audio: streaming ({} frames/period, {} bytes/period, pre-buffered {} packets)",
        FRAMES_PER_PERIOD, stereo_bytes, q.queue.len()
    );

    loop {
        q.fill_stereo_s16le(&mut tx_buf, FRAMES_PER_PERIOD);
        if let Err(e) = i2s_tx.write(&tx_buf, 50) {
            warn!("I2S TX write failed: {:?}", e);
        }
        if let Err(e) = i2s_rx.read(&mut rx_buf, 50) {
            warn!("I2S RX read failed: {:?}", e);
        }
    }
}

/// BOX-3 I2S + clocking for 16 kHz stereo Philips slots (same as `cyber-hub`).
pub fn box3_i2s_std_config() -> i2s::config::StdConfig {
    let slot_config = i2s::config::StdSlotConfig::philips_slot_default(
        i2s::config::DataBitWidth::Bits16,
        i2s::config::SlotMode::Stereo,
    )
    .slot_bit_width(i2s::config::SlotBitWidth::Bits16);
    let clk_config = i2s::config::StdClkConfig::from_sample_rate_hz(16_000)
        .mclk_multiple(i2s::config::MclkMultiple::M256);
    let channel_config = i2s::config::Config::default()
        .auto_clear(true)
        .dma_buffer_count(6)
        .frames_per_buffer(FRAMES_PER_PERIOD as u32);
    i2s::config::StdConfig::new(
        channel_config,
        clk_config,
        slot_config,
        i2s::config::StdGpioConfig::default(),
    )
}
