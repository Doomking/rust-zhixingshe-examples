extern crate alloc;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent};

// ------------------------------------------------------------------------------------------------ //
// [后台任务 1]: 管理 Embassy 的底层协议栈
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) {
    // runner.run() 是一个永远不会返回的死循环
    // 它在底层不停地轮询网卡，把接收到的无线电数据转化为 TCP/IP 报文
    runner.run().await
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 2]: Wi-Fi 状态管理机任务，专门负责连上路由器，并在断线时自动重连
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>) {
    info!("Starting wifi task...");
    
    // 初始化客户端的配置（账号密码）
    // 这里用 env!() 宏读取编译时传入环境变量（我们已经隔离到了 .env 文件里），避免明文密码
    let client_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(alloc::string::String::from(env!("WIFI_SSID")))
            .with_password(alloc::string::String::from(env!("WIFI_PASS")))
    );
    
    // 步骤 1：下发配置到硬件
    controller.set_config(&client_config).expect("Failed to set configuration");

    // 步骤 2：启动 Wi-Fi 硬件（必须在 set_config 之后执行，否则会报错 UnknownWifiMode）
    match controller.start_async().await {
        Ok(_) => info!("Wifi started!"),
        Err(e) => {
            error!("Failed to start wifi: {:?}", defmt::Debug2Format(&e));
            return;
        }
    }

    // 步骤 3：进入无限循环，保持网络一直在线
    loop {
        info!("Connecting to WiFi...");
        // 尝试连接并挂起当前协程，直到返回结果
        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect: {:?}", defmt::Debug2Format(&e));
                Timer::after(Duration::from_millis(5000)).await;
                continue; // 失败了就等 5 秒，重新再试一次
            }
        }

        // 挂起等待硬件抛出 [WifiEvent::StaDisconnected] 断线事件
        controller.wait_for_event(WifiEvent::StaDisconnected).await;
        info!("WiFi disconnected. Reconnecting...");
        // 断线之后继续执行，回到 loop 的开头重新 connect_async()
    }
}
