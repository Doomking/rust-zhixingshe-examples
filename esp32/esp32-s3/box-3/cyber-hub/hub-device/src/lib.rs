#![no_std]
extern crate alloc;

pub mod wifi;
pub mod tcp;
pub mod display;
pub mod imu;
pub mod audio;
pub mod sntp;
pub mod weather;
pub mod fonts;
pub mod sensor;

use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use core::cell::RefCell;

#[derive(Debug, Clone, Copy, Default, defmt::Format)]
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
}

// 定义一个容量为 8 的静态异步队列，用于在 IMU 任务和 TCP 任务间传递“翻转锁屏”事件
pub static FLIP_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, bool, 8> = Channel::new();

// 全局状态真相来源 (守护全局指标)
pub static STATUS_STATE: Mutex<CriticalSectionRawMutex, RefCell<SystemStatus>> = 
    Mutex::new(RefCell::new(SystemStatus { 
        cpu_usage: 0, 
        mem_usage: 0, 
        temperature: 0, 
        local_temp: 0,
        local_hum: 0,
        wind_speed: 0,
        weather_code: 0, 
        city_name: [0u8; 16],
        weather_desc_en: [0u8; 16],
        unix_time: 0 
    }));

// 状态更新通知信号 (仅用于通知 UI 刷新)
pub static SYSTEM_STATUS: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// 音频流频道：用于传输原始 PCM 数据块 (512 字节). 容量扩容为 64 (32KB)，恰好容纳完整的一管 DMA 爆发，防止爆音和 OOM
pub static AUDIO_STREAM_CHANNEL: Channel<CriticalSectionRawMutex, [u8; 512], 64> = Channel::new();
