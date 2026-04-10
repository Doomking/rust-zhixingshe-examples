pub mod display;
pub mod fonts;
pub mod wifi;
pub mod imu;
pub mod sensor;
pub mod tcp;
pub mod audio;
pub mod weather;

use std::sync::{Arc, RwLock};
use std::sync::mpsc::{Sender, Receiver, channel};
use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStatus {
    pub cpu_usage: u8,
    pub mem_usage: u8,
    pub temperature: i8,    // 远程天气温度
    pub local_temp: i8,     // 底座传感器温度
    pub local_hum: u8,      // 底座传感器湿度
    pub wind_speed: u8,     // 风速 (km/h)
    pub weather_code: u16,
    pub city_name: [u8; 16],
    pub weather_desc_en: [u8; 16], // 英文状况描述 (如 "Sunny")
    pub unix_time: u64,
    pub voice_state: u8,    // 0: Idle, 1: Listening, 2: Processing
}

pub static FLIP_EVENT_CHANNEL: OnceLock<(Mutex<Sender<bool>>, Mutex<Receiver<bool>>)> = OnceLock::new();

pub fn get_flip_channel() -> &'static (Mutex<Sender<bool>>, Mutex<Receiver<bool>>) {
    FLIP_EVENT_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

pub static STATUS_STATE: OnceLock<Arc<RwLock<SystemStatus>>> = OnceLock::new();

pub fn get_status() -> Arc<RwLock<SystemStatus>> {
    STATUS_STATE.get_or_init(|| Arc::new(RwLock::new(SystemStatus::default()))).clone()
}

pub static AUDIO_STREAM_CHANNEL: OnceLock<(Mutex<Sender<Vec<u8>>>, Mutex<Receiver<Vec<u8>>>)> = OnceLock::new();

pub fn get_audio_channel() -> &'static (Mutex<Sender<Vec<u8>>>, Mutex<Receiver<Vec<u8>>>) {
    AUDIO_STREAM_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

pub static VOICE_EVENT_CHANNEL: OnceLock<(Mutex<Sender<u32>>, Mutex<Receiver<u32>>)> = OnceLock::new();

pub fn get_voice_channel() -> &'static (Mutex<Sender<u32>>, Mutex<Receiver<u32>>) {
    VOICE_EVENT_CHANNEL.get_or_init(|| {
        let (tx, rx) = channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}
