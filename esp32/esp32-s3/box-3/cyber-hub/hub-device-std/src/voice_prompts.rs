//! 16 kHz, mono, signed 16-bit LE PCM（与 I2S / 上行格式一致）。
//! 内容由 `build.rs` 根据 `.env` 中 `CYBER_HUB_VOICE_WAKE` / `CYBER_HUB_VOICE_DONE` 在编译前生成。

pub const WAKE_ACK_PCM: &[u8] = include_bytes!("../assets/wake.pcm");
pub const COMMAND_DONE_PCM: &[u8] = include_bytes!("../assets/done.pcm");
