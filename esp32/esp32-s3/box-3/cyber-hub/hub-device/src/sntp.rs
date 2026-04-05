use defmt::{info, warn};
use embassy_net::udp::{UdpSocket, PacketMetadata};
use embassy_net::{IpAddress, Ipv4Address, Stack};
use embassy_time::{Duration, Timer};
use crate::{SYSTEM_STATUS, STATUS_STATE};

// NTP 服务器地址: pool.ntp.org
const NTP_SERVER: IpAddress = IpAddress::Ipv4(Ipv4Address::new(162, 159, 200, 1)); // cloudflare ntp
const NTP_PORT: u16 = 123;

#[embassy_executor::task]
pub async fn ntp_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buffer = [0u8; 64];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buffer = [0u8; 64];

    loop {
        // 1. 等待网络就绪
        while !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
        }

        let mut socket = UdpSocket::new(
            stack,
            &mut rx_meta,
            &mut rx_buffer,
            &mut tx_meta,
            &mut tx_buffer,
        );

        let mut ntp_rx_payload = [0u8; 64];

        if let Err(e) = socket.bind(0) {
            warn!("NTP: Failed to bind socket: {:?}", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        info!("NTP: Requesting time from pool.ntp.org...");
        
        // 构建 NTP 请求包 (48 字节)
        // LI=0, VN=3, Mode=3 (Client)
        let mut ntp_packet = [0u8; 48];
        ntp_packet[0] = 0x1b;

        if let Err(e) = socket.send_to(&ntp_packet, (NTP_SERVER, NTP_PORT)).await {
            warn!("NTP: Failed to send request: {:?}", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(10)).await;
            continue;
        }

        // 等待响应
        match embassy_time::with_timeout(Duration::from_secs(5), socket.recv_from(&mut ntp_rx_payload)).await {
            Ok(Ok((n, _addr))) if n >= 48 => {
                // 提取 Transmit Timestamp (字节 40-43)
                // NTP 时间是从 1900-01-01 开始的秒数
                let seconds = u32::from_be_bytes([
                    ntp_rx_payload[40],
                    ntp_rx_payload[41],
                    ntp_rx_payload[42],
                    ntp_rx_payload[43],
                ]);

                // 转换为 Unix 时间 (从 1970-01-01 开始) 并增加 8 小时北京偏移
                let unix_time = seconds as u64 - 2208988800 + 28800;

                // 更新全局状态 (直接锁定 Mutex)
                {
                    let state = STATUS_STATE.lock().await;
                    let mut status = state.borrow_mut();
                    status.unix_time = unix_time;
                }
                // 发送刷新信号给 UI
                SYSTEM_STATUS.signal(());

                info!("[NTP] Synchronized! Beijing Time: {}", unix_time);
                
                // 同步成功后，每小时同步一次
                Timer::after(Duration::from_secs(3600)).await;
            }
            _ => {
                warn!("NTP: Response timeout or invalid packet.");
                Timer::after(Duration::from_secs(10)).await;
            }
        }
    }
}
