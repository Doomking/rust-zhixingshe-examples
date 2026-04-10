use anyhow::Result;
use std::process::Command;
use tokio::fs;
use tracing::{info, error};

/// 将文本转换为 PCM 音频 (16kHz, 16bit, Mono)
/// 使用 macOS 自带的 'say' 命令实现，无需额外依赖
pub async fn text_to_speech(text: &str) -> Result<Vec<u8>> {
    let temp_wav = format!("/tmp/tts_{}.wav", std::process::id());
    
    // 使用 say 命令生成指定格式的音频
    // I16 = 16-bit signed integer
    // @16000 = 16kHz sampling rate
    let status = Command::new("say")
        .arg("-o")
        .arg(&temp_wav)
        .arg("--data-format=I16@16000")
        .arg(text)
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("macOS 'say' command failed"));
    }

    // 读取生成的 WAV 文件
    let mut data = fs::read(&temp_wav).await?;
    
    // 简单粗暴但有效地剥离 WAV 文件头 (通常为 44 字节)
    // 我们的目标是下发原始 PCM 流
    let pcm_data = if data.len() > 44 {
        data.drain(0..44);
        data
    } else {
        data
    };

    // 清理临时文件
    let _ = fs::remove_file(&temp_wav).await;

    info!("[TTS] Generated {} bytes of PCM for: \"{}\"", pcm_data.len(), text);
    Ok(pcm_data)
}
