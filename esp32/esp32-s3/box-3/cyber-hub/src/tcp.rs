use core::str::FromStr;
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

// ------------------------------------------------------------------------------------------------ //
// [后台任务 3]: TCP 客户端任务，负责和 Mac 上的 Server 建立持续稳定连接
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn tcp_client_task(stack: Stack<'static>) {
    // 预先给 Socket 分配在静态内存区的接收(RX)和发送(TX)的缓冲区
    let mut rx_buffer = [0; 1024];
    let mut tx_buffer = [0; 1024];

    loop {
        // 如果物理链路没连上，或者 DHCP 还没获取到通信所需的 IP 地址，就等待 500ms 后重检
        if !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        // 创建一个基于当前的协议栈的 TCP Socket
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10))); // 设置超时时间

        // 将环境变量编译时传进来的 MAC_IP (你的电脑IP) 解析为原生的 Ipv4Address 结构
        let mac_ip = embassy_net::Ipv4Address::from_str(env!("MAC_IP")).expect("Invalid MAC_IP");
        // 组装目标服务端的 IP 和 Port 端点
        let endpoint = embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(mac_ip), 8080);
        
        info!("Connecting to TCP server at {}...", env!("MAC_IP"));
        
        // 发起异步连接，这行代码会让出 CPU 执行权，直到网络连接成功/失败才会苏醒
        if let Err(e) = socket.connect(endpoint).await {
            warn!("TCP connect error: {:?}", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(2)).await; // 连接失败就喘口气重试
            continue;
        }

        info!("TCP connected to Mac!");
        
        let msg = b"Hello Mac from Cyber-Hub!";
        if let Err(e) = socket.write(msg).await {
            warn!("TCP write error: {:?}", defmt::Debug2Format(&e));
            continue;
        } else {
            info!("Sent Hello Mac!");
        }

        // 保持连接，死循环等待 IMU 传来的翻转事件
        loop {
            // 协程会在这里挂起，完全不占用 CPU，直到 channel 收到数据
            let _ = crate::FLIP_EVENT_CHANNEL.receive().await;
            
            let cmd = b"lock_screen\n";
            info!("Sending lock_screen payload via TCP!");
            
            if let Err(e) = socket.write(cmd).await {
                warn!("TCP write error during lock_screen: {:?}", defmt::Debug2Format(&e));
                break; // 写入失败代表断线，跳出内部循环，触发外层的重新连接
            }
        }

        socket.close();
        Timer::after(Duration::from_secs(5)).await;
    }
}
