mod protocol;
mod media;

use protocol::{ControlAck, ControlHello, FrameHeader, FrameType, MediaParams, AvSyncParams, PROTOCOL_VERSION, CONTROL_PAYLOAD_LEN};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use std::time::Instant;
use std::path::PathBuf;

fn build_control_ack(cfg: &media::MediaConfig, client_av: AvSyncParams, client_version: u16) -> ControlAck {
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
    // Load runtime config from `mac-server/.env` (same style as `esp-client/.env`).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let env_path = manifest_dir.join(".env");
    let _ = dotenvy::from_filename(&env_path);

    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr).await?;
    println!("Mac Server listening on {}", addr);

    loop {
        let (mut socket, peer_addr) = listener.accept().await?;
        println!("Accepted connection from {}", peer_addr);

        tokio::spawn(async move {
            let mp4_path = std::env::var("LIQUIDCAST_MP4_PATH")
                .unwrap_or_else(|_| "input.mp4".to_string());

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

            // ---- Control plane handshake (best-effort, backwards compatible) ----
            // If client sends HELLO, reply with ACK containing the negotiated params.
            let mut header_buf = [0u8; FrameHeader::SIZE];
            let mut did_handshake = false;
            let mut hello_av = AvSyncParams {
                drop_late_ms: 120,
                wait_ahead_ms: 40,
            };
            let mut hello_ver = PROTOCOL_VERSION;
            if socket.read_exact(&mut header_buf).await.is_ok() {
                if let Some(h) = FrameHeader::deserialize(&header_buf) {
                    if h.frame_type == FrameType::ControlHello && h.payload_len as usize == CONTROL_PAYLOAD_LEN {
                        let mut payload = vec![0u8; CONTROL_PAYLOAD_LEN];
                        if socket.read_exact(&mut payload).await.is_ok() {
                            if let Some(hello) = ControlHello::deserialize(&payload) {
                                hello_av = hello.av;
                                hello_ver = hello.version;
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
                    } else {
                        // Not a HELLO: treat what we read as the first media header (legacy mode).
                        // We can't "unread" the bytes; so legacy clients must send no data before server starts.
                        // In practice our esp-client always initiates control hello after this upgrade.
                        // If needed later, we can buffer and prepend into rx loop.
                    }
                }
            }
            if did_handshake {
                println!("Handshake OK with {}", peer_addr);
            } else {
                println!("Handshake skipped (legacy) with {}", peer_addr);
            }

            let (tx, mut rx) = mpsc::channel::<(FrameHeader, Vec<u8>)>(4);
            let tx_video = tx.clone();
            let video_cfg = cfg.clone();
            let audio_cfg = cfg.clone();
            let tx_audio = tx.clone();
            let tx_ping = tx.clone();

            let _video_handle = tokio::spawn(async move {
                if let Err(e) = media::spawn_video_task(video_cfg, tx_video).await {
                    eprintln!("[media] video task ended with error: {e}");
                }
            });
            let _audio_handle = tokio::spawn(async move {
                if let Err(e) = media::spawn_audio_task(audio_cfg, tx_audio).await {
                    eprintln!("[media] audio task ended with error: {e}");
                }
            });
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

            let _start_time = Instant::now();
            let mut sent_video = 0u64;
            let mut sent_audio = 0u64;
            let mut sent_ctrl = 0u64;
            let mut sent_bytes = 0u64;
            let mut stat_last = Instant::now();

            while let Some((header, payload)) = rx.recv().await {
                let header_bytes = header.serialize();
                if socket.write_all(&header_bytes).await.is_err() {
                    break;
                }
                if socket.write_all(&payload).await.is_err() {
                    break;
                }
                sent_bytes += (header_bytes.len() + payload.len()) as u64;
                match header.frame_type {
                    FrameType::VideoJpeg => sent_video += 1,
                    FrameType::AudioPcm => sent_audio += 1,
                    FrameType::ControlHello | FrameType::ControlAck | FrameType::ControlPing => sent_ctrl += 1,
                    _ => {}
                }
                if stat_last.elapsed().as_secs() >= 2 {
                    println!(
                        "[session {}] 2s stats: video={} audio={} ctrl={} bytes={} KB",
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

            println!("Connection to {} closed", peer_addr);
        });
    }
}
