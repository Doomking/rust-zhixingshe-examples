use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use log::*;

pub fn connect_wifi<'a>(
    modem: Modem<'a>,
    sysloop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
) -> anyhow::Result<Box<EspWifi<'a>>> {
    let mut wifi = Box::new(EspWifi::new(modem, sysloop.clone(), Some(nvs))?);
    
    let mut b_wifi = BlockingWifi::wrap(&mut *wifi, sysloop)?;

    let wifi_configuration = Configuration::Client(ClientConfiguration {
        ssid: env!("WIFI_SSID").try_into().unwrap(),
        password: env!("WIFI_PASS").try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    b_wifi.set_configuration(&wifi_configuration)?;

    b_wifi.start()?;
    info!("WiFi started");

    b_wifi.connect()?;
    info!("WiFi connected");

    b_wifi.wait_netif_up()?;
    info!("WiFi network interface is up");

    Ok(wifi)
}
