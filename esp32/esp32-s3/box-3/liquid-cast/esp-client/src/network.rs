use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use std::net::TcpStream;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use log::{info, error, warn};
use crossbeam_channel::Sender;

static LOGGED_FIRST_AUDIO_PCM: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
enum SessionState {
    Connecting,
    Handshaking,
    Streaming,
}

#[derive(Debug, Clone)]
pub struct VideoPacket {
    pub timestamp_ms: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub timestamp_ms: u32,
    pub payload: Vec<u8>,
}

pub struct NetworkManager<'a> {
    _wifi: EspWifi<'a>,
}

impl<'a> NetworkManager<'a> {
    pub fn connect_wifi(
        modem: Modem<'a>,
        sysloop: EspSystemEventLoop,
        nvs: EspDefaultNvsPartition,
        ssid: &str,
        password: &str,
    ) -> anyhow::Result<Self> {
        let mut wifi = EspWifi::new(modem, sysloop.clone(), Some(nvs))?;

        wifi.set_configuration(&Configuration::Client(ClientConfiguration {
            ssid: ssid.try_into().unwrap(),
            password: password.try_into().unwrap(),
            auth_method: AuthMethod::WPA2Personal,
            ..Default::default()
        }))?;

        info!("Starting Wi-Fi...");
        wifi.start()?;
        info!("Connecting to Wi-Fi...");
        wifi.connect()?;

        // Wait for connection and IP address
        while !wifi.is_up()? {
            let config = wifi.get_configuration()?;
            info!("Waiting for Wi-Fi to be up (acquire IP)... {:?}", config);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        
        info!("Wi-Fi connected and IP acquired successfully");

        Ok(Self { _wifi: wifi })
    }
}

pub fn start_tcp_client(
    server_addr: &str,
    video_tx: Sender<VideoPacket>,
    audio_tx: Sender<AudioPacket>,
) -> anyhow::Result<()> {
    let mut state = SessionState::Connecting;
    info!("Session state: {:?}", state);
    info!("Connecting to TCP server at {}", server_addr);
    let mut stream = TcpStream::connect(server_addr)?;
    info!("Connected to server!");

    // ---- Control plane: send HELLO, best-effort wait for ACK (backwards compatible) ----
    state = SessionState::Handshaking;
    info!("Session state: {:?}", state);
    let hello = crate::protocol::ControlHello {
        version: crate::protocol::PROTOCOL_VERSION,
        media: crate::protocol::MediaParams {
            video_w: 320,
            video_h: 240,
            video_fps: 20,
            jpeg_q: 3,
            audio_sample_rate: 16_000,
            audio_chunk_bytes: 2048,
        },
        av: crate::protocol::AvSyncParams {
            drop_late_ms: crate::sync::drop_late_ms() as i16,
            wait_ahead_ms: crate::sync::wait_ahead_ms() as i16,
        },
    };
    let hello_payload = hello.serialize();
    let hello_header = crate::protocol::FrameHeader {
        frame_type: crate::protocol::FrameType::ControlHello,
        timestamp_ms: 0,
        payload_len: hello_payload.len() as u32,
    };
    let _ = stream.write_all(&hello_header.serialize());
    let _ = stream.write_all(&hello_payload);

    // Small timeout to avoid stalling legacy servers.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
    let mut ack_header_buf = [0u8; 12];
    if stream.read_exact(&mut ack_header_buf).is_ok() {
        if let Some(h) = crate::protocol::FrameHeader::deserialize(&ack_header_buf) {
            if h.frame_type == crate::protocol::FrameType::ControlAck
                && h.payload_len as usize == crate::protocol::CONTROL_PAYLOAD_LEN
            {
                let mut payload = vec![0u8; crate::protocol::CONTROL_PAYLOAD_LEN];
                if stream.read_exact(&mut payload).is_ok() {
                    if let Some(ack) = crate::protocol::ControlAck::deserialize(&payload) {
                        crate::sync::set_params(
                            ack.av.drop_late_ms as i32,
                            ack.av.wait_ahead_ms as i32,
                        );
                        info!(
                            "Handshake ACK: v{} video={}x{}@{} q={} audio={}Hz chunk={} drop_late={} wait_ahead={}",
                            ack.version,
                            ack.media.video_w,
                            ack.media.video_h,
                            ack.media.video_fps,
                            ack.media.jpeg_q,
                            ack.media.audio_sample_rate,
                            ack.media.audio_chunk_bytes,
                            ack.av.drop_late_ms,
                            ack.av.wait_ahead_ms
                        );
                    }
                }
            } else {
                // Not an ACK: likely a legacy server immediately streaming media.
                // We intentionally ignore this first header; media will continue afterwards.
            }
        }
    }
    // Restore normal read timeout for streaming.
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    state = SessionState::Streaming;
    info!("Session state: {:?}", state);

    // Set read timeout
    let mut header_buf = [0u8; 12];
    
    let mut total_bytes = 0;
    let mut start_time = std::time::Instant::now();
    let mut cnt_video = 0u32;
    let mut cnt_audio = 0u32;
    let mut cnt_ctrl = 0u32;
    let mut cnt_other = 0u32;

    loop {
        // Read header
        if let Err(e) = stream.read_exact(&mut header_buf) {
            error!("Failed to read header: {}", e);
            break;
        }

        if let Some(header) = crate::protocol::FrameHeader::deserialize(&header_buf) {
            // Read payload
            let len = header.payload_len as usize;
            
            if len > 200_000 { // Reasonable max limit for frame
                warn!("Payload too large: {}, dropping connection", len);
                break;
            }
            
            let mut payload_buf = vec![0u8; len];
            if let Err(e) = stream.read_exact(&mut payload_buf) {
                error!("Failed to read payload: {}", e);
                break;
            }

            total_bytes += 12 + len;

            match header.frame_type {
                crate::protocol::FrameType::VideoJpeg => {
                    cnt_video += 1;
                    // Send payload to main thread
                    if video_tx
                        .send(VideoPacket {
                            timestamp_ms: header.timestamp_ms,
                            payload: payload_buf,
                        })
                        .is_err()
                    {
                        warn!("Video channel closed");
                        break;
                    }
                }
                crate::protocol::FrameType::AudioPcm => {
                    cnt_audio += 1;
                    if !LOGGED_FIRST_AUDIO_PCM.swap(true, Ordering::Relaxed) {
                        info!(
                            "AudioPcm: first chunk received ({} bytes) — stream has audio",
                            len
                        );
                    }
                    if audio_tx
                        .send(AudioPacket {
                            timestamp_ms: header.timestamp_ms,
                            payload: payload_buf,
                        })
                        .is_err()
                    {
                        warn!("Audio channel closed");
                        break;
                    }
                }
                crate::protocol::FrameType::ControlPing => {
                    cnt_ctrl += 1;
                }
                crate::protocol::FrameType::ControlAck => {
                    cnt_ctrl += 1;
                    if let Some(ack) = crate::protocol::ControlAck::deserialize(&payload_buf) {
                        crate::sync::set_params(
                            ack.av.drop_late_ms as i32,
                            ack.av.wait_ahead_ms as i32,
                        );
                        info!(
                            "Runtime ACK: drop_late={} wait_ahead={}",
                            ack.av.drop_late_ms, ack.av.wait_ahead_ms
                        );
                    }
                }
                _ => {
                    cnt_other += 1;
                }
            }

            // Log stats every second (counts avoid bias: last frame in the window is often VideoJpeg)
            if start_time.elapsed().as_secs() >= 1 {
                info!(
                    "Stream 1s: video={} audio={} ctrl={} other={} | last: {:?} ts={} len={} | {} KB/s",
                    cnt_video,
                    cnt_audio,
                    cnt_ctrl,
                    cnt_other,
                    header.frame_type,
                    header.timestamp_ms,
                    len,
                    total_bytes / 1024
                );
                cnt_video = 0;
                cnt_audio = 0;
                cnt_ctrl = 0;
                cnt_other = 0;
                total_bytes = 0;
                start_time = std::time::Instant::now();
            }
        } else {
            error!("Invalid header received, dropping connection");
            break;
        }
    }
    
    Ok(())
}
