//! Phase 3.2 — Mac 端「环境通讯网关」：监听 BOX-3 的 TCP 连接，解析 `0x5A` 帧协议，
//! 将语音 PCM 交给 [`crate::audio::AudioProcessor`]，再经 STT → [`crate::ai::AiProcessor`] 转发至 ZeroClaw（OpenAI 兼容 Chat Completions）。

use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::ai::AiProcessor;
use crate::audio::AudioProcessor;
use crate::config::AppConfig;
use crate::system::control::trigger_macos_lock;
use crate::system::metrics::MetricsMonitor;

pub async fn handle_device_connection(
    socket: TcpStream,
    config: AppConfig,
    metrics: std::sync::Arc<MetricsMonitor>,
    ai_processor: std::sync::Arc<AiProcessor>,
) -> Result<()> {
    let (mut rd, wr) = socket.into_split();
    let wr = std::sync::Arc::new(Mutex::new(wr));

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let port = rd.peer_addr().map(|a| a.port()).unwrap_or(0);
    let session_id = format!("{}_{}", timestamp, port);

    let mut audio_processor = AudioProcessor::new(&config, session_id.clone());

    let storage_base = std::path::Path::new(&config.audio_storage_path);
    if !storage_base.exists() {
        std::fs::create_dir_all(storage_base)?;
    }

    let pcm_path = storage_base.join(format!("audio_{}.pcm", session_id));
    let mut file = File::create(&pcm_path).await?;

    let wr_metrics = wr.clone();
    let metrics_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let (cpu, mem) = metrics.get_usage().await;
            let packet = [
                crate::protocol::MAGIC_HEADER,
                crate::protocol::MSG_METRICS,
                0x02,
                0x00,
                cpu,
                mem,
            ];
            let mut g = wr_metrics.lock().await;
            if let Err(e) = g.write_all(&packet).await {
                warn!("Metrics send failed: {}", e);
                break;
            }
        }
    });

    let mut buffer = [0u8; 4096];
    let mut leftover = Vec::with_capacity(8192);

    loop {
        let n = match rd.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(e.into()),
        };
        leftover.extend_from_slice(&buffer[..n]);

        while leftover.len() >= 4 {
            if leftover[0] != crate::protocol::MAGIC_HEADER {
                leftover.remove(0);
                continue;
            }

            let msg_type = leftover[1];
            let payload_len = u16::from_le_bytes([leftover[2], leftover[3]]) as usize;

            if leftover.len() < 4 + payload_len {
                break;
            }

            let packet_data: Vec<u8> = leftover.drain(..4 + payload_len).collect();
            let payload = &packet_data[4..];

            match msg_type {
                crate::protocol::MSG_METRICS => {}
                crate::protocol::MSG_FLIP_EVENT => {
                    info!("\x1b[31;1m[PKT] Lock Screen Command Received!\x1b[0m");
                    trigger_macos_lock();
                }
                crate::protocol::MSG_VOICE_START => {
                    info!("\x1b[32;1m[PKT] Voice Wakeup Detected!\x1b[0m");
                    ai_processor.notify_wakeup();
                    audio_processor.start_manual_session()?;
                }
                crate::protocol::MSG_VOICE_DATA => {
                    if !payload.is_empty() {
                        if let Some(wav_path) = audio_processor.process_data(payload)? {
                            // Refresh wake window at the end of speech so long speech + STT time doesn't expire the window
                            ai_processor.notify_wakeup();
                            let ai_ptr = ai_processor.clone();
                            let wr_ai = wr.clone();
                            tokio::spawn(async move {
                                if let Err(e) = ai_ptr.process_utterance(wav_path, wr_ai).await {
                                    error!("AI Processing Error: {}", e);
                                }
                            });
                        }
                        file.write_all(payload).await?;
                    }
                }
                crate::protocol::MSG_VOICE_END => {
                    info!("[PKT] Voice Session End");
                    if let Some(wav_path) = audio_processor.stop_manual_session()? {
                        // Refresh wake window at the end of speech so long speech + STT time doesn't expire the window
                        ai_processor.notify_wakeup();
                        let ai_ptr = ai_processor.clone();
                        let wr_ai = wr.clone();
                        tokio::spawn(async move {
                            if let Err(e) = ai_ptr.process_utterance(wav_path, wr_ai).await {
                                error!("AI Processing Error: {}", e);
                            }
                        });
                    }
                }
                _ => {
                    warn!("[PKT] Unknown type 0x{:02X}", msg_type);
                }
            }
        }
    }

    metrics_handle.abort();
    info!("Connection closed: {}", session_id);
    Ok(())
}
