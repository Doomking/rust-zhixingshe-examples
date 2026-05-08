//! I2S 双工播放: 单声道 s16le 16 kHz -> 立体声 ES8311，带接收端数据回流保护。

use crossbeam_channel::Receiver;
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{self, I2sBiDir, I2sDriver};
use log::{error, info, warn};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::codec_es8311::es8311_init;
use crate::network::AudioPacket;

/// 软件增益: 在复制单声道到左右声道前进行放大
const PLAYBACK_GAIN: i32 = 3;

/// 每个 I2S 周期的采样帧数 (与服务端发送节奏匹配)
const FRAMES_PER_PERIOD: usize = 512;
const AUDIO_SAMPLE_RATE: u32 = 16_000;
const MS_PER_SAMPLE_FP: u32 = 1_000_000 / AUDIO_SAMPLE_RATE; // 微秒级定点数计算

// 用于 A/V 同步的全局音频时间戳 (毫秒)
static CURRENT_AUDIO_TIME_MS: AtomicU32 = AtomicU32::new(0);

pub fn current_audio_time_ms() -> u32 {
    CURRENT_AUDIO_TIME_MS.load(Ordering::Relaxed)
}

/// PCM 数据队列管理，负责处理网络包到连续音频流的转换
struct PcmQueue {
    rx: Receiver<AudioPacket>,
    queue: VecDeque<AudioPacket>,
    cur: AudioPacket,
    off: usize,
    cur_samples_played: u32,
    have_clock_base: bool,
    last_log: std::time::Instant,
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
            last_log: std::time::Instant::now(),
        }
    }

    /// 从通道接收新包并存入队列
    fn poll_rx(&mut self) {
        while let Ok(v) = self.rx.try_recv() {
            if !v.payload.is_empty() {
                self.queue.push_back(v);
            }
        }
    }

    /// 当前包播放完后，移动到队列中的下一个包
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

    /// 填充 I2S 目标缓冲区: 将单声道数据复制为双声道，并计算当前播放时间戳
    fn fill_stereo_s16le(&mut self, dst: &mut [u8], frames: usize) {
        self.poll_rx();
        
        // 每 2 秒打印一次缓冲区健康状况
        if self.last_log.elapsed().as_secs() >= 2 {
            info!("音频缓冲区状态: 长度={} (约 {}ms 缓存) | 当前时间戳={}", 
                self.queue.len(), 
                self.queue.len() * 64, // 每个 2048 字节包约 64ms
                self.cur.timestamp_ms
            );
            self.last_log = std::time::Instant::now();
        }

        for i in 0..frames {
            self.advance_buffer();
            let m = if self.off + 2 <= self.cur.payload.len() {
                // 读取单声道 s16le
                let raw =
                    i16::from_le_bytes([self.cur.payload[self.off], self.cur.payload[self.off + 1]])
                        as i32;
                self.off += 2;
                self.cur_samples_played = self.cur_samples_played.saturating_add(1);
                // 应用增益并限幅
                (raw * PLAYBACK_GAIN).clamp(i16::MIN as i32, i16::MAX as i32) as i16
            } else {
                0i16 // 无数据时静音
            };
            let b = m.to_le_bytes();
            let idx = i * 4;
            // 复制到左右声道
            dst[idx..idx + 2].copy_from_slice(&b); // L
            dst[idx + 2..idx + 4].copy_from_slice(&b); // R
        }

        // 更新全局音频时钟，供视频渲染线程同步使用
        if self.have_clock_base {
            let delta_ms = ((self.cur_samples_played as u64) * (MS_PER_SAMPLE_FP as u64) / 1000) as u32;
            CURRENT_AUDIO_TIME_MS.store(self.cur.timestamp_ms.saturating_add(delta_ms), Ordering::Relaxed);
        }
    }
}

/// 音频播放线程入口
pub fn run_playback_thread(
    i2s: I2sDriver<'static, I2sBiDir>,
    i2c: Arc<Mutex<I2cDriver<'static>>>,
    audio_rx: Receiver<AudioPacket>,
) {
    info!("音频: I2S 播放线程已启动...");

    let i2s = Box::leak(Box::new(i2s));
    let (mut i2s_rx, mut i2s_tx) = i2s.split();

    let stereo_bytes = FRAMES_PER_PERIOD * 2 * 2;
    let mut tx_buf = vec![0u8; stereo_bytes];
    let mut rx_buf = vec![0u8; stereo_bytes];
    let warmup_silence = vec![0u8; stereo_bytes];

    // 预热 I2S 硬件，防止初次启动时的杂音
    const WARMUP_ITERS: u32 = 24;
    info!("音频: I2S 硬件预热中...");
    for _ in 0..WARMUP_ITERS {
        let _ = i2s_tx.write(&warmup_silence, 10);
        let _ = i2s_rx.read(&mut rx_buf, 1000);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // 初始化 ES8311 芯片 (通过 I2C 设置寄存器)
    if let Ok(mut bus) = i2c.lock() {
        if let Err(e) = es8311_init(&mut *bus) {
            error!("ES8311 初始化失败: {:?}", e);
        }
    } else {
        error!("I2C 锁被污染，ES8311 未配置");
    }

    let mut q = PcmQueue::new(audio_rx);

    // ---- 预缓冲策略: 等待足够的音频包后再开始播放，防止初始卡顿 ----
    const PREBUF_TARGET: usize = 30; // 目标缓冲约 2 秒数据
    const PREBUF_TIMEOUT_MS: u64 = 10_000;
    info!("音频: 正在进行预缓冲 (目标 {} 包)...", PREBUF_TARGET);
    let prebuf_start = std::time::Instant::now();
    loop {
        q.poll_rx();
        if q.queue.len() >= PREBUF_TARGET {
            break;
        }
        if prebuf_start.elapsed().as_millis() as u64 > PREBUF_TIMEOUT_MS {
            warn!("音频: 预缓冲超时，已收到 {} 包，强制开始播放", q.queue.len());
            break;
        }
        // 预缓冲期间输出静音数据，保持 I2S 驱动运行
        let _ = i2s_tx.write(&warmup_silence, 10);
        let _ = i2s_rx.read(&mut rx_buf, 10);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    info!("音频: 流播放上线 (缓冲区已就绪)");

    // ---- 循环播放阶段 ----
    loop {
        q.fill_stereo_s16le(&mut tx_buf, FRAMES_PER_PERIOD);
        // 写入 I2S 发送队列 (DMA 处理)
        if let Err(e) = i2s_tx.write(&tx_buf, 50) {
            warn!("I2S 发送失败: {:?}", e);
        }
        // 必须清空 I2S 接收队列，防止底层缓冲区溢出
        if let Err(e) = i2s_rx.read(&mut rx_buf, 50) {
            warn!("I2S 接收清理失败: {:?}", e);
        }
    }
}

/// BOX-3 I2S 标准配置: 16 kHz 立体声，Philips 格式
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
