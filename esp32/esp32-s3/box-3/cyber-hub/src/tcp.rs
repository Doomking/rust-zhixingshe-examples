use core::str::FromStr;
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

// ------------------------------------------------------------------------------------------------ //
// [后台任务 3]: TCP 客户端任务 (增强稳定性版本)
// 策略：持久连接 + 指令重传 + 心跳保活
// - 解决了 Mac 锁屏导致连接重置 (Connection Reset) 时事件丢失的问题。
// - 如果发送失败，会立即重新连接并重试该指令，直到成功送达。
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn tcp_client_task(stack: Stack<'static>) {
    let mac_ip = embassy_net::Ipv4Address::from_str(env!("MAC_IP")).expect("Invalid MAC_IP");
    let endpoint = embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(mac_ip), 8080);

    let mut rx_buffer = [0u8; 512];
    let mut tx_buffer = [0u8; 512];

    'main: loop {
        // 1. 等待网络层就绪
        while !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
        }

        // 2. 建立新连接
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));
        socket.set_keep_alive(Some(Duration::from_secs(5))); // 5秒心跳，探测死掉的连接

        info!("Connecting to Mac TCP server...");
        if let Err(e) = socket.connect(endpoint).await {
            warn!("TCP connect failed: {:?}. Retrying in 2s...", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(2)).await;
            continue 'main;
        }
        info!("TCP connected! Ready for events.");

        // 3. 事件处理循环
        loop {
            // A. 阻塞等待翻转事件
            let _ = crate::FLIP_EVENT_CHANNEL.receive().await;
            info!("Flip event popped from channel, sending...");

            // B. 尝试写入指令
            // 我们用一个内部循环来保证“必须送达”
            'delivery: loop {
                if socket.write(b"lock_screen\n").await.is_ok() {
                    if socket.flush().await.is_ok() {
                        info!("lock_screen sent successfully!");
                        // 冷却 1 秒防抖
                        Timer::after(Duration::from_secs(1)).await;
                        break 'delivery; // 指令成功送达，继续等待下一个事件
                    }
                }

                // C. 如果写入/Flush失败，说明连接已被 Mac 重置 (Reset by peer)
                warn!("TCP Write failed (Mac might have locked). Reconnecting and retrying...");
                drop(socket);
                Timer::after(Duration::from_millis(500)).await;

                // 尝试重新建立连接
                let mut new_socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
                new_socket.set_timeout(Some(Duration::from_secs(10)));
                if let Err(e) = new_socket.connect(endpoint).await {
                    warn!("Retry-connect failed: {:?}. Waiting 1s...", defmt::Debug2Format(&e));
                    Timer::after(Duration::from_secs(1)).await;
                    socket = new_socket;
                    continue 'delivery; // 继续重试当前指令
                }
                socket = new_socket;
                info!("TCP Reconnected! Retrying data transmission...");
                // 重新连接成功后，'delivery 循环会回到顶部再次尝试 socket.write
            }
        }
    }
}
