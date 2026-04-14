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
    if let Some(m) = PLAYBACK_TX.get() {
        if let Ok(g) = m.lock() {
            let _ = g.send(mono_s16le);
        }
    }
}
