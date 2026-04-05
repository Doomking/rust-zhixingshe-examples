use core::str::FromStr;
use defmt::{info, warn};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use embassy_futures::select::select;
use embedded_io_async::Write;

// ------------------------------------------------------------------------------------------------ //
// [后台任务 3]: TCP 客户端任务 (读写分离加速版本)
// 策略：利用 socket.split() 彻底解耦音频发送与指标接收
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn tcp_client_task(stack: Stack<'static>) {
    let mac_ip = embassy_net::Ipv4Address::from_str(env!("MAC_IP")).expect("Invalid MAC_IP");
    let endpoint = embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(mac_ip), 8080);

    let mut rx_buffer = [0u8; 1024];
    let mut tx_buffer = [0u8; 1024];

    'main: loop {
        // 1. 网络就绪检查
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
        info!("Connected to Hub Server (Dual-Stream Mode)!");

        // 3. 读写分离逻辑
        let (mut reader, mut writer) = socket.split();

        // 子任务 A: 接收服务端指标 (解决拆包/分包问题)
        let read_fut = async {
            let mut metrics_accumulator = [0u8; 4];
            let mut metrics_ptr = 0;
            loop {
                match reader.read(&mut metrics_accumulator[metrics_ptr..]).await {
                    Ok(0) => {
                        warn!("TCP Reader: Server closed connection.");
                        return; // 结束内层 read 循环
                    }
                    Ok(n) => {
                        metrics_ptr += n;
                        if metrics_ptr >= 4 {
                            if metrics_accumulator[0] == 0x01 {
                                let cpu = metrics_accumulator[1];
                                let mem = metrics_accumulator[2];
                                {
                                    let state = crate::STATUS_STATE.lock().await;
                                    let mut status = state.borrow_mut();
                                    status.cpu_usage = cpu;
                                    status.mem_usage = mem;
                                }
                                crate::SYSTEM_STATUS.signal(());
                                info!("[METRICS] Final Success: CPU: {}%, Mem: {}%", cpu, mem);
                            } else {
                                // 包头错误，平移重校准
                                metrics_accumulator.copy_within(1..4, 0);
                                metrics_ptr = 3;
                                continue;
                            }
                            metrics_ptr = 0;
                        }
                    }
                    Err(e) => {
                        warn!("TCP Reader: Read error: {:?}", defmt::Debug2Format(&e));
                        return;
                    }
                }
            }
        };

        // 子任务 B: 发送音频流与指令 (全力输出)
        let write_fut = async {
            loop {
                let res = select(
                    crate::FLIP_EVENT_CHANNEL.receive(),
                    crate::AUDIO_STREAM_CHANNEL.receive()
                ).await;

                match res {
                    embassy_futures::select::Either::First(_) => {
                        info!("TCP Writer: Sending lock_screen...");
                        if writer.write_all(b"lock_screen\n").await.is_err() {
                            warn!("TCP Writer: Command failed.");
                            return;
                        }
                    }
                    embassy_futures::select::Either::Second(pcm_data) => {
                        if writer.write_all(&pcm_data).await.is_err() {
                            warn!("TCP Writer: Audio stream failed.");
                            return;
                        }
                    }
                }
            }
        };

        // 并行执行读写，任何一方退出则整体重连
        let _ = select(read_fut, write_fut).await;
        
        info!("TCP Session finished. Reconnecting in 1s...");
        Timer::after(Duration::from_secs(1)).await;
    }
}
