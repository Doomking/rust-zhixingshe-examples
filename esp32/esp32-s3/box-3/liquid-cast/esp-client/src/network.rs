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
    Connecting,  // 正在连接服务器
    Handshaking, // 正在进行协议握手
    Streaming,   // 正在接收流媒体数据
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
    /// 连接到指定的 Wi-Fi 网络
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

        info!("正在启动 Wi-Fi...");
        wifi.start()?;
        info!("正在连接 Wi-Fi...");
        wifi.connect()?;

        // 等待连接成功并获取 IP 地址
        while !wifi.is_up()? {
            let config = wifi.get_configuration()?;
            info!("正在等待 IP 分配... {:?}", config);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        
        info!("Wi-Fi 连接成功，IP 已分配");

        Ok(Self { _wifi: wifi })
    }
}

/// 流同步恢复机制: 在数据错位时搜索 Magic Word ("LC") 重新定位帧头
fn resync_stream(stream: &mut TcpStream) -> anyhow::Result<[u8; 12]> {
    warn!("流同步丢失，正在搜索起始魔数 (Magic Word)...");
    let mut window = [0u8; 2];
    stream.read_exact(&mut window)?;

    loop {
        if window[0] == 0x4C && window[1] == 0x43 { // "L" 和 "C"
            info!("找到起始魔数！正在重新同步...");
            let mut header_rest = [0u8; 10];
            stream.read_exact(&mut header_rest)?;
            let mut full_header = [0u8; 12];
            full_header[0..2].copy_from_slice(&window);
            full_header[2..12].copy_from_slice(&header_rest);
            return Ok(full_header);
        }
        // 滑动窗口继续搜索
        window[0] = window[1];
        let mut next_byte = [0u8; 1];
        stream.read_exact(&mut next_byte)?;
        window[1] = next_byte[0];
    }
}

/// 启动 TCP 客户端并进行流数据分发
pub fn start_tcp_client(
    server_addr: &str,
    video_tx: Sender<VideoPacket>,
    audio_tx: Sender<AudioPacket>,
) -> anyhow::Result<()> {
    let mut state = SessionState::Connecting;
    info!("会话状态: {:?}", state);
    info!("正在连接 TCP 服务器: {}", server_addr);
    let mut stream = TcpStream::connect(server_addr)?;
    info!("服务器连接成功！");

    // ---- 协议握手阶段: 发送设备参数并等待服务器确认 ----
    state = SessionState::Handshaking;
    info!("会话状态: {:?}", state);
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

    // 等待服务器的 ACK (确认) 响应
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
                        // 根据服务器反馈动态调整 A/V 同步参数
                        crate::sync::set_params(
                            ack.av.drop_late_ms as i32,
                            ack.av.wait_ahead_ms as i32,
                        );
                        info!(
                            "握手成功: v{} 视频={}x{}@{} q={} 音频={}Hz 缓冲区={}/{}ms",
                            ack.version, ack.media.video_w, ack.media.video_h, ack.media.video_fps,
                            ack.media.jpeg_q, ack.media.audio_sample_rate, ack.av.drop_late_ms, ack.av.wait_ahead_ms
                        );
                    }
                }
            }
        }
    }
    
    // 设置正常的数据读取超时
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    state = SessionState::Streaming;
    info!("会话状态: {:?}", state);

    let mut header_buf = [0u8; 12];
    let mut total_bytes = 0;
    let mut start_time = std::time::Instant::now();
    let mut cnt_video = 0u32;
    let mut cnt_audio = 0u32;
    let mut cnt_ctrl = 0u32;
    let mut cnt_other = 0u32;

    // ---- 数据接收主循环 ----
    loop {
        // 1. 读取 12 字节的帧头
        if let Err(e) = stream.read_exact(&mut header_buf) {
            error!("无法读取帧头: {}", e);
            break;
        }

        // 2. 解析帧头并处理可能的同步丢失
        let header = match crate::protocol::FrameHeader::deserialize(&header_buf) {
            Some(h) => h,
            None => {
                match resync_stream(&mut stream) {
                    Ok(new_buf) => {
                        header_buf = new_buf;
                        crate::protocol::FrameHeader::deserialize(&header_buf).unwrap()
                    }
                    Err(e) => {
                        error!("重新同步失败: {}", e);
                        break;
                    }
                }
            }
        };

        // 3. 读取负载数据 (Payload)
        let len = header.payload_len as usize;
        if len > 200_000 { // 防止异常大的数据包导致内存溢出
            warn!("负载数据过大: {}, 尝试重新同步", len);
            match resync_stream(&mut stream) {
                Ok(new_buf) => {
                    header_buf = new_buf;
                    continue;
                }
                Err(e) => {
                    error!("大负载后的重新同步失败: {}", e);
                    break;
                }
            }
        }

        let mut payload_buf = vec![0u8; len];
        if let Err(e) = stream.read_exact(&mut payload_buf) {
            error!("无法读取负载数据: {}", e);
            break;
        }

        total_bytes += 12 + len;

        // 4. 根据帧类型分发数据
        match header.frame_type {
            crate::protocol::FrameType::VideoJpeg => {
                cnt_video += 1;
                let _ = video_tx.send(VideoPacket {
                    timestamp_ms: header.timestamp_ms,
                    payload: payload_buf,
                });
            }
            crate::protocol::FrameType::AudioPcm => {
                cnt_audio += 1;
                if !LOGGED_FIRST_AUDIO_PCM.swap(true, Ordering::Relaxed) {
                    info!("音频流已上线: 收到首个 PCM 包 ({} bytes)", len);
                }
                let _ = audio_tx.send(AudioPacket {
                    timestamp_ms: header.timestamp_ms,
                    payload: payload_buf,
                });
            }
            crate::protocol::FrameType::ControlPing => {
                cnt_ctrl += 1;
            }
            crate::protocol::FrameType::ControlAck => {
                cnt_ctrl += 1;
                // 服务器运行期间也可能下发新的 A/V 同步参数
                if let Some(ack) = crate::protocol::ControlAck::deserialize(&payload_buf) {
                    crate::sync::set_params(
                        ack.av.drop_late_ms as i32,
                        ack.av.wait_ahead_ms as i32,
                    );
                    info!(
                        "实时同步更新: 延迟容忍={}ms 休眠阈值={}ms",
                        ack.av.drop_late_ms, ack.av.wait_ahead_ms
                    );
                }
            }
            _ => {
                cnt_other += 1;
            }
        }

        // 5. 每秒打印一次统计信息 (FPS, 码率等)
        if start_time.elapsed().as_secs() >= 1 {
            info!(
                "流监控 (1s): 视频={} 音频={} 控制={} 其他={} | 码率: {} KB/s",
                cnt_video, cnt_audio, cnt_ctrl, cnt_other, total_bytes / 1024
            );
            cnt_video = 0;
            cnt_audio = 0;
            cnt_ctrl = 0;
            cnt_other = 0;
            total_bytes = 0;
            start_time = std::time::Instant::now();
        }
    }

    Ok(())
}
