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

// 音频流频道：用于传输原始 PCM 数据块 (512 字节). 容量扩容为 64 (32KB)，恰好容纳完整的一管 DMA 爆发，防止爆音和 OOM
pub static AUDIO_STREAM_CHANNEL: Channel<CriticalSectionRawMutex, [u8; 512], 64> = Channel::new();
