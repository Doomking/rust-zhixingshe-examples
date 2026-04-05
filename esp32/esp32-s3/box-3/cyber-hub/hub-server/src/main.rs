use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = TcpListener::bind(&addr).await?;
    info!("CyberHub Server (Mac) starting on {}...", addr);
    
    // 初始化系统监控
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );
    // 首次刷新以获取基准值
    sys.refresh_all();
    let sys = std::sync::Arc::new(tokio::sync::Mutex::new(sys));

    loop {
        let (socket, addr) = listener.accept().await?;
        info!("Accepted connection from {}", addr);
        let sys_clone = sys.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, sys_clone).await {
                error!("Error handling connection from {}: {}", addr, e);
            }
        });
    }
}

async fn handle_connection(socket: TcpStream, sys: std::sync::Arc<tokio::sync::Mutex<System>>) -> Result<()> {
    // 1. 初始化 AI 客户端 (保留原有逻辑，尽管暂时没用到)
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
    info!("Saving continuous audio stream to: {}", filename);

    // 分离读写流
    let (mut rd, mut wr) = socket.into_split();

    // 任务 A: 定期采集并发送系统指标到设备
    let metrics_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            
            let (cpu_usage, mem_usage) = {
                let mut s = sys.lock().await;
                s.refresh_all();
                
                let cpu = s.global_cpu_usage() as u8;
                let mem = (s.used_memory() as f64 / s.total_memory() as f64 * 100.0) as u8;
                (cpu, mem)
            };

            info!("[METRICS SEND] CPU: {}%, MEM: {}%", cpu_usage, mem_usage);

            // 协议设计：[0x01: Type][CPU: u8][Mem: u8][Padding: 0x00]
            let packet = [0x01u8, cpu_usage, mem_usage, 0x00u8];
            if let Err(e) = wr.write_all(&packet).await {
                warn!("Failed to send metrics: {}. Stopping metrics loop.", e);
                break;
            }
        }
    });

    // 任务 B: 接收来自设备的消息 (PCM 或者控制指令)
    let mut buffer = [0u8; 2048];
    loop {
        let n = rd.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        let data = &buffer[..n];
        if data.windows(12).any(|w| w == b"lock_screen\n") {
            info!("[COMMAND] lock_screen received from device!");
            trigger_macos_lock();
        } else {
            // 收到音频流块，持续保存到文件并记录
            info!(
                "[AUDIO] Received {} bytes. Offset file: {}. Snippet: {:02x?}",
                n,
                filename,
                &data[..16.min(n)]
            );
            file.write_all(data).await?;
        }
    }

    metrics_handle.abort();
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
