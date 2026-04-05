#![no_std]
#![no_main]
#![deny(clippy::mem_forget, reason = "在 esp_hal 中，内存泄漏是非常危险的。")]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Config, StackResources};
use embassy_time::{Duration, Timer, Instant};
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;

use cyber_hub::audio::{Es8311, Es7210, audio_record_task, dummy_tx_task};
use cyber_hub::display::{draw_dashboard, draw_static_ui, draw_cyber_hub_ui};
use cyber_hub::imu::imu_task;
use cyber_hub::sensor::sensor_task;
use cyber_hub::wifi::{net_task, wifi_task};
use cyber_hub::tcp::tcp_client_task;
use cyber_hub::sntp::ntp_task;
use cyber_hub::weather::weather_task;

use esp_backtrace as _;
extern crate alloc;
esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    rtt_target::rtt_init_defmt!();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);
    info!("Embassy initialized!");

    // --- 网络 ---
    let radio_init = alloc::boxed::Box::leak(alloc::boxed::Box::new(esp_radio::init().expect("Wifi init")));
    let (wifi_controller, interfaces) = esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default()).expect("Wifi controller");
    let mut rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (stack, runner) = embassy_net::new(interfaces.sta, Config::dhcpv4(Default::default()), mk_static!(StackResources<10>, StackResources::<10>::new()), seed);

    // --- 屏幕 ---
    use esp_hal::gpio::{Level, Output, OutputConfig};
    use esp_hal::spi::master::Spi;
    let dc = Output::new(peripherals.GPIO4, Level::Low, OutputConfig::default());
    let cs = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let mut rst = Output::new(peripherals.GPIO48, Level::High, OutputConfig::default());
    let mut backlight = Output::new(peripherals.GPIO47, Level::Low, OutputConfig::default());
    let _pa_ctrl = Output::new(peripherals.GPIO46, Level::High, OutputConfig::default());

    let spi = Spi::new(peripherals.SPI2, esp_hal::spi::master::Config::default().with_frequency(esp_hal::time::Rate::from_mhz(40)))
        .expect("SPI").with_sck(peripherals.GPIO7).with_mosi(peripherals.GPIO6);

    struct ChunkedSpiBus<B>(B);
    impl<B: embedded_hal::spi::ErrorType> embedded_hal::spi::ErrorType for ChunkedSpiBus<B> { type Error = B::Error; }
    impl<B: embedded_hal::spi::SpiBus<u8>> embedded_hal::spi::SpiBus<u8> for ChunkedSpiBus<B> {
        fn read(&mut self, words: &mut [u8]) -> Result<(), B::Error> { self.0.read(words) }
        fn write(&mut self, words: &[u8]) -> Result<(), B::Error> {
            for chunk in words.chunks(60) { self.0.write(chunk)?; }
            Ok(())
        }
        fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), B::Error> { self.0.transfer(read, write) }
        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), B::Error> { self.0.transfer_in_place(words) }
        fn flush(&mut self) -> Result<(), B::Error> { self.0.flush() }
    }

    let spi_device = embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(ChunkedSpiBus(spi), cs).expect("SPI");
    let di = display_interface_spi::SPIInterface::new(spi_device, dc);
    let mut delay = esp_hal::delay::Delay::new();
    rst.set_high(); delay.delay_millis(20u32); rst.set_low(); delay.delay_millis(150u32);
    let mut display = mipidsi::Builder::new(mipidsi::models::ILI9342CRgb565, di)
        .orientation(mipidsi::options::Orientation::new().rotate(mipidsi::options::Rotation::Deg180))
        .color_order(mipidsi::options::ColorOrder::Bgr)
        .init(&mut delay)
        .expect("Display Init Failed");
    delay.delay_millis(100u32); // 给屏幕 100ms 稳定期，防止启动白屏
    backlight.set_high();

    // --- I2S ---
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_circular_buffers!(32768, 4096);
    let i2s = esp_hal::i2s::master::I2s::new(peripherals.I2S0, peripherals.DMA_CH0, esp_hal::i2s::master::Config::default()
            .with_data_format(esp_hal::i2s::master::DataFormat::Data16Channel16)
            .with_sample_rate(esp_hal::time::Rate::from_hz(16000)),
    ).expect("I2S Init").into_async().with_mclk(peripherals.GPIO2);
    let i2s_rx = i2s.i2s_rx.with_din(peripherals.GPIO16).build(rx_descriptors);
    let i2s_tx = i2s.i2s_tx.with_bclk(peripherals.GPIO17).with_ws(peripherals.GPIO45).with_dout(peripherals.GPIO15).build(tx_descriptors);

    // --- I2C ---
    use embedded_hal_bus::i2c::CriticalSectionDevice;
    use core::cell::RefCell;
    use critical_section::Mutex as CSMutex;
    let i2c_config = esp_hal::i2c::master::Config::default()
        .with_frequency(esp_hal::time::Rate::from_khz(100));
    let i2c0 = I2c::new(peripherals.I2C0, i2c_config)
        .expect("I2C Init")
        .with_sda(peripherals.GPIO8)
        .with_scl(peripherals.GPIO18);
    let i2c_bus = mk_static!(CSMutex<RefCell<I2c<'static, Blocking>>>, CSMutex::new(RefCell::new(i2c0)));

    // --- I2C1: 为底座 Sensor Board 配置 (GPIO 40, 41) ---
    let i2c_config1 = esp_hal::i2c::master::Config::default().with_frequency(esp_hal::time::Rate::from_khz(100));
    let i2c1 = I2c::new(peripherals.I2C1, i2c_config1)
        .expect("I2C1 Init")
        .with_sda(peripherals.GPIO41)
        .with_scl(peripherals.GPIO40);
    let i2c_bus_dock = mk_static!(CSMutex<RefCell<I2c<'static, Blocking>>>, CSMutex::new(RefCell::new(i2c1)));

    // --- 任务派发 ---
    spawner.spawn(net_task(runner)).unwrap();
    spawner.spawn(wifi_task(wifi_controller)).unwrap();
    spawner.spawn(tcp_client_task(stack)).unwrap();
    Timer::after(Duration::from_millis(3500)).await;
    spawner.spawn(dummy_tx_task(i2s_tx, tx_buffer)).unwrap();
    spawner.spawn(audio_record_task(i2s_rx, rx_buffer)).unwrap();

    // --- I2C 设备初始化 ---
    let mut codec_out = Es8311::new(CriticalSectionDevice::new(i2c_bus));
    codec_out.init().await.expect("ES8311 Init");
    let mut codec_in = Es7210::new(CriticalSectionDevice::new(i2c_bus));
    codec_in.init().await.expect("ES7210 Init");
    use icm42670::{Address, Icm42670};
    let imu = Icm42670::new(CriticalSectionDevice::new(i2c_bus), Address::Primary).expect("IMU Init");
    spawner.spawn(imu_task(imu)).unwrap();
    // 使用专用的 I2C1 (GPIO 40/41) 处理底座 AHT30
    spawner.spawn(sensor_task(CriticalSectionDevice::new(i2c_bus_dock))).unwrap();

    spawner.spawn(ntp_task(stack)).unwrap();
    spawner.spawn(weather_task(stack)).unwrap();

    info!("All systems GO! Waiting for IPv4 DHCP...");
    Timer::after(Duration::from_millis(5000)).await; // 宽限 5s 给 DHCP 握手
    loop {
        if stack.is_config_up() { break; }
        Timer::after(Duration::from_millis(500)).await;
    }

    use cyber_hub::{SYSTEM_STATUS, STATUS_STATE};
    use cyber_hub::display::{draw_dashboard, draw_static_ui, draw_time, draw_metrics, draw_weather}; 
    let mut last_status = cyber_hub::SystemStatus::default();
    let mut last_ntp_sync: u64 = 0;
    let mut first_run = true;
    let mut sync_unix_time: u64 = 0;
    let mut sync_instant = Instant::now();
    let mut current_status = cyber_hub::SystemStatus::default();

    draw_static_ui(&mut display).ok();
    
    loop {
        match embassy_futures::select::select(SYSTEM_STATUS.wait(), Timer::after(Duration::from_millis(500))).await {
            embassy_futures::select::Either::First(_) => { 
                let status: cyber_hub::SystemStatus = {
                    let state = STATUS_STATE.lock().await;
                    let cell: &core::cell::RefCell<cyber_hub::SystemStatus> = &*state;
                    *cell.borrow()
                };
                
                // 仅当发现 NTP 同步产生的真实时间跳变时，才重置本地时基
                if status.unix_time != last_ntp_sync && status.unix_time != 0 {
                    last_ntp_sync = status.unix_time;
                    sync_unix_time = status.unix_time;
                    sync_instant = Instant::now();
                    info!("[TIME] Base reset to NTP: {}", sync_unix_time);
                }

                // 保持本地已累加的时间，仅更新其他指标 (CPU/RAM/Temp)
                let local_now = current_status.unix_time;
                current_status = status;
                if sync_unix_time != 0 {
                    current_status.unix_time = local_now;
                }
            }
            embassy_futures::select::Either::Second(_) => {
                if sync_unix_time > 0 {
                    let elapsed = sync_instant.elapsed().as_secs();
                    current_status.unix_time = sync_unix_time + elapsed;
                }
            }
        };

        // --- 增量刷新选择器 (Flicker Control) ---
        let time_changed = (current_status.unix_time / 60) != (last_status.unix_time / 60);
        let metrics_changed = current_status.cpu_usage != last_status.cpu_usage 
            || current_status.mem_usage != last_status.mem_usage
            || current_status.local_temp != last_status.local_temp
            || current_status.local_hum != last_status.local_hum;
        let weather_changed = current_status.weather_code != last_status.weather_code
            || current_status.temperature != last_status.temperature;

        if first_run {
            draw_dashboard(&mut display, &current_status).ok();
            last_status = current_status;
            first_run = false;
        } else {
            if time_changed {
                draw_time(&mut display, current_status.unix_time).ok();
            }
            if metrics_changed {
                info!("[UI] Redrawing metrics: CPU={}%, RAM={}%", current_status.cpu_usage, current_status.mem_usage);
                draw_metrics(&mut display, &current_status).ok();
            }
            if weather_changed {
                info!("[UI] Redrawing weather: Code={}, Temp={}", current_status.weather_code, current_status.temperature);
                draw_weather(&mut display, &current_status).ok();
            }
            last_status = current_status;
        }
    }
}
