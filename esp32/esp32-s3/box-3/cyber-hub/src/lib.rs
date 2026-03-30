#![no_std]

pub mod wifi;
pub mod tcp;
pub mod display;
pub mod imu;

use embassy_sync::channel::Channel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

// 定义一个容量为 2 的静态异步队列，用于在 IMU 任务和 TCP 任务间传递“翻转锁屏”事件
pub static FLIP_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();
