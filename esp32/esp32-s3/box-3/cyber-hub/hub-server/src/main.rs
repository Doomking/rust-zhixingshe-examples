use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = TcpListener::bind(&addr).await?;
    info!("CyberHub Server (Mac) starting on {}...", addr);
    info!(
        "AI Backend: {}",
        std::env::var("AI_BASE_URL").unwrap_or_default()
    );

    loop {
        let (socket, addr) = listener.accept().await?;
        info!("Accepted connection from {}", addr);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket).await {
                error!("Error handling connection from {}: {}", addr, e);
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream) -> Result<()> {
    // 1. 初始化 AI 客户端
    let base_url =
        std::env::var("AI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let api_key = std::env::var("AI_API_KEY").unwrap_or_else(|_| "ollama".to_string());
    let ai_model = std::env::var("AI_MODEL").unwrap_or_else(|_| "llama3".to_string());

    let config = async_openai::config::OpenAIConfig::new()
        .with_api_base(base_url)
        .with_api_key(api_key);
    let _client = async_openai::Client::with_config(config);

    info!("New connection from device. Using AI Model: {}", ai_model);

    // 为当前连接创建唯一的音频日志文件
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let port = socket.peer_addr().map(|a| a.port()).unwrap_or(0);
    let filename = format!("audio_{}_{}.pcm", timestamp, port);
    let mut file = File::create(&filename).await?;
    info!("Saving audio stream to: {}", filename);

    let mut buffer = [0u8; 1024];
    loop {
        let n = socket.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        let data = &buffer[..n];
        if data.windows(12).any(|w| w == b"lock_screen\n") {
            trigger_macos_lock();
        } else {
            // 收到音频流块，保存到文件并记录
            info!(
                "[AUDIO] Received {} bytes. Data snippet: {:02x?}",
                n,
                &data[..16.min(n)]
            );
            file.write_all(data).await?;
        }
    }
    info!("Connection closed");
    Ok(())
}

fn trigger_macos_lock() {
    info!("[COMMAND] Lock Screen triggered!");

    // 逻辑参考 python 的代码，优先尝试 macOS 私有 API (SACLockScreenImmediate)
    let private_api_success = unsafe {
        match libloading::Library::new(
            "/System/Library/PrivateFrameworks/login.framework/Versions/Current/login",
        ) {
            Ok(lib) => match lib.get::<unsafe extern "C" fn()>(b"SACLockScreenImmediate") {
                Ok(lock_func) => {
                    info!("[LOCK] Calling SACLockScreenImmediate via Private API...");
                    lock_func();
                    true
                }
                Err(e) => {
                    warn!("[LOCK] Symbol SACLockScreenImmediate not found: {}", e);
                    false
                }
            },
            Err(e) => {
                warn!("[LOCK] Could not load login.framework: {}", e);
                false
            }
        }
    };

    if !private_api_success {
        info!("[LOCK] Private API Lock failed, trying fallback (pmset)...");
        // 方法 3: 强制显示器进入睡眠（如果设置了唤醒需密码，则等同于锁屏）
        if let Err(e) = std::process::Command::new("pmset")
            .arg("displaysleepnow")
            .spawn()
        {
            error!("Failed to execute pmset: {}", e);
        }
    }
}
