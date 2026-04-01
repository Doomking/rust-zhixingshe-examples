use core::str::FromStr;
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use embassy_futures::select::{select, Either};

// ------------------------------------------------------------------------------------------------ //
// [后台任务 3]: TCP 客户端任务 (音频+事件双流版本)
// 策略：持久连接 + 指令重传 + 音频流式传输
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn tcp_client_task(stack: Stack<'static>) {
    let mac_ip = embassy_net::Ipv4Address::from_str(env!("MAC_IP")).expect("Invalid MAC_IP");
    let endpoint = embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(mac_ip), 8080);

    let mut rx_buffer = [0u8; 1024];
    let mut tx_buffer = [0u8; 1024];

    'main: loop {
        // 1. 等待网络就绪
        while !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
        }

        // 2. 建立连接
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));
        socket.set_keep_alive(Some(Duration::from_secs(5)));

        info!("Connecting to Hub Server at {}...", endpoint);
        if let Err(e) = socket.connect(endpoint).await {
            warn!("Connect failed: {:?}. Retry in 2s...", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(2)).await;
            continue 'main;
        }
        info!("Connected to Hub Server!");

        // 3. 多路复用循环：处理翻转事件 OR 持续发送音频流
        loop {
            // 使用 select! 同时监听两个 Channel
            match select(
                crate::FLIP_EVENT_CHANNEL.receive(),
                crate::AUDIO_STREAM_CHANNEL.receive()
            ).await {
                // 情况一：收到了翻转事件
                Either::First(_) => {
                    info!("TCP: Sending lock_screen command...");
                    if socket.write(b"lock_screen\n").await.is_err() || socket.flush().await.is_err() {
                        warn!("TCP: Command write failed, reconnecting...");
                        break;
                    }
                    info!("TCP: Command sent!");
                    // 强制冷却，防止重传干扰声音
                    Timer::after(Duration::from_millis(500)).await;
                }
                // 情况二：收到了新的音频采样块 (512 字节)
                Either::Second(pcm_data) => {
                    // 直接发送原始 PCM 数据
                    // 为了让上位机区分，我们可以在前面加个微小的 Header，或者通过长度判断
                    // 这里我们采用最简单的策略：直接写入。Server 端靠包大小识别
                    if socket.write(&pcm_data).await.is_err() {
                        warn!("TCP: Audio streaming failed, reconnecting...");
                        break;
                    }
                    // 音频流不需要 flush 每一个包，提高效率
                }
            }
        }

        socket.close();
        drop(socket);
        Timer::after(Duration::from_millis(500)).await;
    }
}
