pub mod audio;
pub mod display;
pub mod fonts;
pub mod imu;
pub mod sensor;
pub mod tcp;
pub mod voice_prompts;
pub mod weather;
pub mod wifi;
pub mod protocol;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStatus {
    pub cpu_usage: u8,
    pub mem_usage: u8,
    pub temperature: i8, // 远程天气温度
    pub local_temp: i8,  // 底座传感器温度
    pub local_hum: u8,   // 底座传感器湿度
    pub wind_speed: u8,  // 风速 (km/h)
    pub weather_code: u16,
    pub city_name: [u8; 16],
    pub weather_desc_en: [u8; 16], // 英文状况描述 (如 "Sunny")
    pub unix_time: u64,
    pub voice_state: u8,    // 0: Idle, 1: Listening, 2: Processing, 3: Replying
    pub mic_rms: u8,        // 能量值 (0-100)，用于 Phase 2 动画
    pub last_activity: u64, // 最后活跃时间，用于 Phase 1 超时判断
}

pub static FLIP_EVENT_CHANNEL: OnceLock<(Mutex<Sender<bool>>, Mutex<Receiver<bool>>)> =
    OnceLock::new();

pub fn get_flip_channel() -> &'static (Mutex<Sender<bool>>, Mutex<Receiver<bool>>) {
    FLIP_EVENT_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

pub static STATUS_STATE: OnceLock<Arc<RwLock<SystemStatus>>> = OnceLock::new();

pub fn get_status() -> Arc<RwLock<SystemStatus>> {
    STATUS_STATE
        .get_or_init(|| Arc::new(RwLock::new(SystemStatus::default())))
        .clone()
}

pub static AUDIO_STREAM_CHANNEL: OnceLock<(Mutex<Sender<Vec<u8>>>, Mutex<Receiver<Vec<u8>>>)> =
    OnceLock::new();

pub fn get_audio_channel() -> &'static (Mutex<Sender<Vec<u8>>>, Mutex<Receiver<Vec<u8>>>) {
    AUDIO_STREAM_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

pub static VOICE_EVENT_CHANNEL: OnceLock<(Mutex<Sender<u32>>, Mutex<Receiver<u32>>)> =
    OnceLock::new();

pub fn get_voice_channel() -> &'static (Mutex<Sender<u32>>, Mutex<Receiver<u32>>) {
    VOICE_EVENT_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

/// 下行播放队列：单声道 s16le 16 kHz PCM；由 `audio_thread` 消费并送到 I2S TX。
static PLAYBACK_TX: OnceLock<Mutex<Sender<Vec<u8>>>> = OnceLock::new();

/// 在此之前不向服务端发送麦克风上行（避免本地喇叭 → 麦克风的回声被当成用户语音）。
static VOICE_UPLINK_SUPPRESS_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn voice_uplink_suppress_slot() -> &'static Mutex<Option<Instant>> {
    VOICE_UPLINK_SUPPRESS_UNTIL.get_or_init(|| Mutex::new(None))
}

/// 当前是否应丢弃送往 `AUDIO_STREAM_CHANNEL` 的 PCM（本地正在播提示音等）。
pub(crate) fn voice_uplink_suppressed() -> bool {
    let Ok(guard) = voice_uplink_suppress_slot().lock() else {
        return false;
    };
    match *guard {
        Some(until) if Instant::now() < until => true,
        _ => false,
    }
}

/// 按 16 kHz mono s16le 长度估算播放时长，并在此后 `extra_ms` 内继续抑制上行。
fn extend_voice_uplink_suppress(mono_s16le_len: usize, extra_ms: u64) {
    let samples = mono_s16le_len / 2;
    let pcm_ms = (samples as u64).saturating_mul(1000) / 16_000;
    let total_ms = pcm_ms.saturating_add(extra_ms).max(1);
    let new_until = Instant::now() + Duration::from_millis(total_ms);
    if let Ok(mut g) = voice_uplink_suppress_slot().lock() {
        *g = Some(match *g {
            Some(prev) => prev.max(new_until),
            None => new_until,
        });
    }
}

/// 须在启动 TCP / `audio_thread` 之前调用一次。
pub fn init_playback_pipe() -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    PLAYBACK_TX
        .set(Mutex::new(tx))
        .expect("init_playback_pipe: call only once");
    rx
}

pub fn enqueue_playback_pcm(mono_s16le: Vec<u8>) {
    if mono_s16le.is_empty() {
        return;
    }
    // 防止「我在」/ done 提示被麦克风采到并上传（无 AEC 时的实用折中）。
    extend_voice_uplink_suppress(mono_s16le.len(), 80);
    if let Some(m) = PLAYBACK_TX.get() {
        if let Ok(g) = m.lock() {
            let _ = g.send(mono_s16le);
        }
    }
}
