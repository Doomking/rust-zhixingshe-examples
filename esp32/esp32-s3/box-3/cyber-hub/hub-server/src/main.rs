use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use hub_server::config::AppConfig;
use hub_server::system::control::trigger_macos_lock;
use hub_server::system::metrics::MetricsMonitor;
use hub_server::ai::AiProcessor;
use hub_server::audio::AudioProcessor;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::from_env();
    let addr = format!("0.0.0.0:{}", config.port);

    let ai_processor = std::sync::Arc::new(AiProcessor::new(&config).await);
    let metrics = std::sync::Arc::new(MetricsMonitor::new());

    let listener = TcpListener::bind(&addr).await?;
    info!("CyberHub Server (Mac) starting on {}...", addr);
    
    loop {
        let (socket, addr) = listener.accept().await?;
        info!("Accepted connection from {}", addr);
        let config_clone = config.clone();
        let metrics_clone = metrics.clone();
        let ai_clone = ai_processor.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, config_clone, metrics_clone, ai_clone).await {
                error!("Error handling connection from {}: {}", addr, e);
            }
        });
    }
}

async fn handle_connection(
    socket: TcpStream, 
    config: AppConfig, 
    metrics: std::sync::Arc<MetricsMonitor>,
    ai_processor: std::sync::Arc<AiProcessor>
) -> Result<()> {
    let (mut rd, mut wr) = socket.into_split();
    
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let port = rd.peer_addr().map(|a| a.port()).unwrap_or(0);
    let session_id = format!("{}_{}", timestamp, port);

    let mut audio_processor = AudioProcessor::new(&config, session_id.clone());
    
    // Industrial Path Management
    let storage_base = std::path::Path::new(&config.audio_storage_path);
    if !storage_base.exists() {
        std::fs::create_dir_all(storage_base)?;
    }
    
    let pcm_path = storage_base.join(format!("audio_{}.pcm", session_id));
    let mut file = File::create(&pcm_path).await?;

    // Task A: Periodic Metrics (Server -> Device)
    let metrics_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let (cpu, mem) = metrics.get_usage().await;
            // Unified 0x5A Packet: [Magic, Type, LenL, LenH, CPU, MEM]
            let packet = [
                hub_server::protocol::MAGIC_HEADER, 
                hub_server::protocol::MSG_METRICS, 
                0x02, 0x00, cpu, mem
            ];
            if let Err(e) = wr.write_all(&packet).await {
                warn!("Metrics send failed: {}", e);
                break;
            }
        }
    });

    // Task B: Receiver (Device -> Server)
    // Industrial Packet Decoder
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
            // Find Magic Byte
            if leftover[0] != hub_server::protocol::MAGIC_HEADER {
                leftover.remove(0);
                continue;
            }

            let msg_type = leftover[1];
            let payload_len = u16::from_le_bytes([leftover[2], leftover[3]]) as usize;

            if leftover.len() < 4 + payload_len {
                break; // Wait for more data
            }

            // Extract payload
            let packet_data: Vec<u8> = leftover.drain(..4+payload_len).collect();
            let payload = &packet_data[4..];

            match msg_type {
                hub_server::protocol::MSG_METRICS => { // Metrics
                    // Handled locally
                }
                hub_server::protocol::MSG_FLIP_EVENT => { // Flip Event
                    info!("\x1b[31;1m[PKT] Lock Screen Command Received!\x1b[0m");
                    trigger_macos_lock();
                }
                hub_server::protocol::MSG_VOICE_START => { // Wakeup Start
                    info!("\x1b[32;1m[PKT] Voice Wakeup Detected!\x1b[0m");
                    ai_processor.notify_wakeup(); // Authorize the upcoming session
                    audio_processor.start_manual_session()?;
                }
                hub_server::protocol::MSG_VOICE_DATA => { // Audio Chunk
                    // Only process audio if payload exists
                    if !payload.is_empty() {
                        if let Some(wav_path) = audio_processor.process_data(payload)? {
                            let ai_ptr = ai_processor.clone();
                            tokio::spawn(async move {
                                if let Err(e) = ai_ptr.process_utterance(wav_path).await {
                                    error!("AI Processing Error: {}", e);
                                }
                            });
                        }
                        file.write_all(payload).await?;
                    }
                }
                hub_server::protocol::MSG_VOICE_END => { // Voice End
                    info!("[PKT] Voice Session End");
                    if let Some(wav_path) = audio_processor.stop_manual_session()? {
                        let ai_ptr = ai_processor.clone();
                        tokio::spawn(async move {
                            if let Err(e) = ai_ptr.process_utterance(wav_path).await {
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
