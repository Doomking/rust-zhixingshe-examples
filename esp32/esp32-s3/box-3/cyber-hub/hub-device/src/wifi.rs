extern crate alloc;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent};

// ------------------------------------------------------------------------------------------------ //
// [后台任务 1]: 管理 Embassy 的底层协议栈
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 2]: Wi-Fi 状态管理机任务，专门负责连上路由器，并在断线时自动重连
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>) {
    info!("Starting wifi task...");
    
    // 初始化客户端的配置（账号密码）
    let client_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(alloc::string::String::from(env!("WIFI_SSID")))
            .with_password(alloc::string::String::from(env!("WIFI_PASS")))
    );
    
    controller.set_config(&client_config).expect("Failed to set configuration");

    match controller.start_async().await {
        Ok(_) => info!("Wifi started!"),
        Err(e) => {
            error!("Failed to start wifi: {:?}", defmt::Debug2Format(&e));
            return;
        }
    }

    // [调优点 2]：禁用功耗管理 (Power Save)。
    // 默认的 Modem Sleep 可能会在 CPU 负载较高时导致硬件中断延迟，从而引发 wifi_internal_tx 挂死。
    // 在实时音频传输场景下，必须保持无线电始终唤醒。
    use esp_radio::wifi::PowerSaveMode;
    if let Err(e) = controller.set_power_saving(PowerSaveMode::None) {
        info!("Note: Power save configuration skip or fail: {:?}", defmt::Debug2Format(&e));
    }

    loop {
        info!("Connecting to WiFi...");
        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect: {:?}", defmt::Debug2Format(&e));
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
        }

        controller.wait_for_event(WifiEvent::StaDisconnected).await;
        info!("WiFi disconnected. Reconnecting...");
    }
}
