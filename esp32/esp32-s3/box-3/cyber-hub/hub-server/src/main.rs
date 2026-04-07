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

    let listener = TcpListener::bind(&addr).await?;
    info!("CyberHub Server (Mac) starting on {}...", addr);
    
    let metrics = std::sync::Arc::new(MetricsMonitor::new());

    loop {
        let (socket, addr) = listener.accept().await?;
        info!("Accepted connection from {}", addr);
        let config_clone = config.clone();
        let metrics_clone = metrics.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, config_clone, metrics_clone).await {
                error!("Error handling connection from {}: {}", addr, e);
            }
        });
    }
}

async fn handle_connection(
    socket: TcpStream, 
    config: AppConfig, 
    metrics: std::sync::Arc<MetricsMonitor>
) -> Result<()> {
    // 实例化核心处理器
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let port = socket.peer_addr().map(|a| a.port()).unwrap_or(0);
    let session_id = format!("{}_{}", timestamp, port);

    let ai_processor = std::sync::Arc::new(AiProcessor::new(&config));
    let mut audio_processor = AudioProcessor::new(&config, session_id.clone());

    // 原始 PCM 持续记录文件
    let filename = format!("audio_{}.pcm", session_id);
    let mut file = File::create(&filename).await?;

    let (mut rd, mut wr) = socket.into_split();

    // 任务 A: 定期发送系统指标
    let metrics_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let (cpu, mem) = metrics.get_usage().await;
            let packet = [0x01u8, cpu, mem, 0x00u8];
            if let Err(e) = wr.write_all(&packet).await {
                warn!("Failed to send metrics: {}", e);
                break;
            }
        }
    });

    // 任务 B: 接收数据并分发
    let mut buffer = [0u8; 2048];
    loop {
        let n = match rd.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(e.into()),
        };

        let data = &buffer[..n];
        if data.windows(12).any(|w| w == b"lock_screen\n") {
            trigger_macos_lock();
        } else {
            // 音频处理核心
            if let Some(wav_path) = audio_processor.process_data(data)? {
                let ai_ptr = ai_processor.clone();
                tokio::spawn(async move {
                    if let Err(e) = ai_ptr.process_utterance(wav_path).await {
                        error!("AI Processing Error: {}", e);
                    }
                });
            }
            file.write_all(data).await?;
        }
    }

    metrics_handle.abort();
    info!("Connection closed: {}", session_id);
    Ok(())
}
