#![no_std]

pub mod wifi;
pub mod tcp;
pub mod display;
pub mod imu;
pub mod audio;

use embassy_sync::channel::Channel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

// 定义一个容量为 2 的静态异步队列，用于在 IMU 任务和 TCP 任务间传递“翻转锁屏”事件
pub static FLIP_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();

// [动态内存重构]：移除静态 AUDIO_STREAM_CHANNEL 以腾挪 SRAM。
// 我们改用类型定义，在 main.rs 中动态分配到 PSRAM 堆中。
pub type AudioChannel = Channel<CriticalSectionRawMutex, [u8; 512], 64>;
