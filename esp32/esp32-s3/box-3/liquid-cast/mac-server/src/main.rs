mod protocol;
mod media;

use protocol::{ControlAck, ControlHello, FrameHeader, FrameType, MediaParams, AvSyncParams, PROTOCOL_VERSION, CONTROL_PAYLOAD_LEN};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use std::time::Instant;
use std::path::PathBuf;

/// 构造握手确认包 (ACK)，包含媒体参数和 A/V 同步配置
fn build_control_ack(cfg: &media::MediaConfig, client_av: AvSyncParams, client_version: u16) -> ControlAck {
    // 优先从环境变量读取 A/V 同步参数，否则沿用客户端默认值
    let drop_late_ms: i16 = std::env::var("LIQUIDCAST_DROP_LATE_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(client_av.drop_late_ms);
    let wait_ahead_ms: i16 = std::env::var("LIQUIDCAST_WAIT_AHEAD_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(client_av.wait_ahead_ms);

    ControlAck {
        version: PROTOCOL_VERSION.max(client_version),
        media: MediaParams {
            video_w: cfg.video_width as u16,
            video_h: cfg.video_height as u16,
            video_fps: cfg.video_fps as u16,
            jpeg_q: cfg.jpeg_q,
            audio_sample_rate: cfg.audio_sample_rate,
            audio_chunk_bytes: cfg.audio_chunk_bytes as u16,
        },
        av: AvSyncParams { drop_late_ms, wait_ahead_ms },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从 mac-server 目录下的 .env 文件加载环境变量
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let env_path = manifest_dir.join(".env");
    let _ = dotenvy::from_filename(&env_path);

    // 绑定 TCP 监听端口
    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr).await?;
    println!("Mac 服务端已启动，监听地址: {}", addr);

    loop {
        // 等待 ESP 客户端连接
        let (mut socket, peer_addr) = listener.accept().await?;
        println!("接受来自 {} 的连接", peer_addr);

        // 为每个连接创建一个独立的异步任务
        tokio::spawn(async move {
            let mp4_path = std::env::var("LIQUIDCAST_MP4_PATH")
                .unwrap_or_else(|_| "input.mp4".to_string());

            // 基础流媒体配置
            let cfg = media::MediaConfig {
                mp4_path,
                video_width: 320,
                video_height: 240,
                video_fps: std::env::var("LIQUIDCAST_FPS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(20),
                jpeg_q: std::env::var("LIQUIDCAST_JPEG_Q")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3),
                audio_sample_rate: 16000,
                audio_chunk_bytes: 2048,
                debug_save_first_frames: 3,
                debug_save_audio_seconds: 3,
            };

            // ---- 协议握手阶段 ----
            let mut header_buf = [0u8; FrameHeader::SIZE];
            let mut did_handshake = false;
            let mut hello_av = AvSyncParams {
                drop_late_ms: 120,
                wait_ahead_ms: 40,
            };
            let mut hello_ver = PROTOCOL_VERSION;

            // 尝试读取客户端发送的 HELLO 帧
            if socket.read_exact(&mut header_buf).await.is_ok() {
                if let Some(h) = FrameHeader::deserialize(&header_buf) {
                    if h.frame_type == FrameType::ControlHello && h.payload_len as usize == CONTROL_PAYLOAD_LEN {
                        let mut payload = vec![0u8; CONTROL_PAYLOAD_LEN];
                        if socket.read_exact(&mut payload).await.is_ok() {
                            if let Some(hello) = ControlHello::deserialize(&payload) {
                                hello_av = hello.av;
                                hello_ver = hello.version;
                                // 发送 ACK 确认
                                let ack = build_control_ack(&cfg, hello.av, hello.version);
                                let ack_payload = ack.serialize();
                                let ack_header = FrameHeader {
                                    frame_type: FrameType::ControlAck,
                                    timestamp_ms: 0,
                                    payload_len: ack_payload.len() as u32,
                                };
                                let _ = socket.write_all(&ack_header.serialize()).await;
                                let _ = socket.write_all(&ack_payload).await;
                                did_handshake = true;
                            }
                        }
                    }
                }
            }

            if did_handshake {
                println!("与 {} 握手成功", peer_addr);
            } else {
                println!("与 {} 握手跳过 (旧版本客户端)", peer_addr);
            }

            // 创建内部 MPSC 通道，用于收集视频、音频、控制任务的数据并统一通过 TCP 发送
            let (tx, mut rx) = mpsc::channel::<(FrameHeader, Vec<u8>)>(4);
            let tx_video = tx.clone();
            let video_cfg = cfg.clone();
            let audio_cfg = cfg.clone();
            let tx_audio = tx.clone();
            let tx_ping = tx.clone();

            // 启动视频处理任务: 调用 ffmpeg 解码视频帧
            let _video_handle = tokio::spawn(async move {
                if let Err(e) = media::spawn_video_task(video_cfg, tx_video).await {
                    eprintln!("[media] 视频任务异常终止: {e}");
                }
            });

            // 启动音频处理任务: 调用 ffmpeg 提取 PCM 音频
            let _audio_handle = tokio::spawn(async move {
                if let Err(e) = media::spawn_audio_task(audio_cfg, tx_audio).await {
                    eprintln!("[media] 音频任务异常终止: {e}");
                }
            });

            // 心跳任务: 每 2 秒发送一个心跳包
            let _ping_handle = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    ticker.tick().await;
                    let ping = FrameHeader {
                        frame_type: FrameType::ControlPing,
                        timestamp_ms: 0,
                        payload_len: 0,
                    };
                    if tx_ping.send((ping, Vec::new())).await.is_err() {
                        break;
                    }
                }
            });

            // A/V 参数同步任务: 每 5 秒推流一次同步参数 (用于运行时动态调整)
            let tx_cfg = tx.clone();
            let cfg_for_ctrl = cfg.clone();
            let _cfg_push_handle = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    ticker.tick().await;
                    let ack = build_control_ack(&cfg_for_ctrl, hello_av, hello_ver);
                    let payload = ack.serialize().to_vec();
                    let header = FrameHeader {
                        frame_type: FrameType::ControlAck,
                        timestamp_ms: 0,
                        payload_len: payload.len() as u32,
                    };
                    if tx_cfg.send((header, payload)).await.is_err() {
                        break;
                    }
                }
            });

            // 数据外发统计
            let mut sent_video = 0u64;
            let mut sent_audio = 0u64;
            let mut sent_ctrl = 0u64;
            let mut sent_bytes = 0u64;
            let mut stat_last = Instant::now();

            // ---- TCP 数据外发循环 ----
            while let Some((header, payload)) = rx.recv().await {
                let header_bytes = header.serialize();
                // 写入 12 字节帧头
                if socket.write_all(&header_bytes).await.is_err() {
                    break;
                }
                // 写入负载数据
                if socket.write_all(&payload).await.is_err() {
                    break;
                }
                sent_bytes += (header_bytes.len() + payload.len()) as u64;
                match header.frame_type {
                    FrameType::VideoJpeg => sent_video += 1,
                    FrameType::AudioPcm => sent_audio += 1,
                    _ => sent_ctrl += 1,
                }
                // 每 2 秒输出一次服务端统计信息
                if stat_last.elapsed().as_secs() >= 2 {
                    println!(
                        "[会话 {}] 2s 统计: 视频={} 音频={} 控制={} | 吞吐: {} KB/s",
                        peer_addr,
                        sent_video,
                        sent_audio,
                        sent_ctrl,
                        sent_bytes / 1024
                    );
                    sent_video = 0;
                    sent_audio = 0;
                    sent_ctrl = 0;
                    sent_bytes = 0;
                    stat_last = Instant::now();
                }
            }

            println!("与 {} 的连接已关闭", peer_addr);
        });
    }
}
