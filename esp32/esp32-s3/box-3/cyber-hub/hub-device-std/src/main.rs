use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported
use log::*;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::{config::Config as SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig};
use esp_idf_hal::gpio::*;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::units::FromValueType;
use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
use mipidsi::options::{ColorOrder, Orientation, Rotation};
use mipidsi::models::ILI9342CRgb565;
use cyber_hub_std::{
    display::{draw_cyber_hub_ui, draw_dashboard, flush_framebuffer, FrameBuffer},
    wifi::connect_wifi,
    imu::imu_thread,
    sensor::sensor_thread,
    tcp::tcp_thread,
    audio::{Es7210, audio_thread},
    weather::weather_thread,
    get_status,
};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_hal::{i2c, i2s};
use icm42670::{Icm42670, Address};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Suppress legacy I2C driver warning from ESP-IDF C layer
    unsafe {
        esp_idf_sys::esp_log_level_set(
            b"i2c\0".as_ptr() as *const _,
            esp_idf_sys::esp_log_level_t_ESP_LOG_ERROR,
        );
    }

    info!("Booting Cyber-Hub (std architecture, ESP-IDF)...");

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // --- 1. SPI Display Initialization ---
    // LCD Pins: SCK=7, MOSI=6, CS=5, DC=4, RST=48, BL=47
    let spi = SpiDriver::new(
        peripherals.spi2,
        pins.gpio7,      // SCK
        pins.gpio6,      // MOSI
        None::<AnyIOPin>, // MISO
        &SpiDriverConfig::new(),
    )?;

    // SPI Config: 40MHz for LCD
    let spi_config = SpiConfig::new().baudrate(40.MHz().into());
    let spi_device = SpiDeviceDriver::new(spi, Some(pins.gpio5), &spi_config)?; // CS = GPIO5
    
    let dc = PinDriver::output(pins.gpio4)?;
    let mut rst = PinDriver::output(pins.gpio48)?;
    let mut backlight = PinDriver::output(pins.gpio47)?;

    // SPI Interface
    let di = display_interface_spi::SPIInterface::new(spi_device, dc);
    
    // Hardware Reset
    rst.set_high()?; Ets::delay_ms(20); rst.set_low()?; Ets::delay_ms(150);

    // Initializing MIPIDSI
    let mut display = mipidsi::Builder::new(ILI9342CRgb565, di)
        .orientation(Orientation::new().rotate(Rotation::Deg180))
        .color_order(ColorOrder::Bgr)
        .init(&mut Ets)
        .expect("Display Init Failed");

    Ets::delay_ms(100);
    backlight.set_high()?;

    info!("Display initialized successfully.");
    draw_cyber_hub_ui(&mut display, "System Migrating...")
        .expect("Draw UI");

    // --- 2. WiFi Initialization ---
    draw_cyber_hub_ui(&mut display, "Connecting to WiFi...")
        .expect("Draw UI");
    let _wifi = connect_wifi(peripherals.modem, sysloop.clone(), nvs.clone())?;
    
    draw_cyber_hub_ui(&mut display, "WiFi Connected! \nSyncing Time...")
        .expect("Draw UI");

    // --- 2b. SNTP Time Sync ---
    let _sntp = EspSntp::new_default()?;
    info!("SNTP initialized, waiting for time sync...");
    for _ in 0..20 {
        if _sntp.get_sync_status() == SyncStatus::Completed {
            info!("SNTP time synchronized!");
            break;
        }
        thread::sleep(std::time::Duration::from_millis(500));
    }

    // --- 3. I2C Initialization ---
    // I2C0: SDA=8, SCL=18 (IMU + Codecs)
    let i2c0_config = i2c::I2cConfig::new().baudrate(400.kHz().into());
    let mut i2c0 = i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c0_config)?;
    
    // I2C1: SDA=41, SCL=40 (Dock Sensor)
    let i2c1_config = i2c::I2cConfig::new().baudrate(100.kHz().into());
    let i2c1 = i2c::I2cDriver::new(peripherals.i2c1, pins.gpio41, pins.gpio40, &i2c1_config)?;

    // Initialize Codecs (Temporary use of I2C0)
    {
        let mut es7210 = Es7210::new(&mut i2c0);
        es7210.init().expect("ES7210 Init");
    }

    // --- 4. I2S Initialization (16kHz, Master, PCM) ---
    let i2s_config = i2s::config::StdConfig::new(
        i2s::config::Config::new(),
        i2s::config::StdClkConfig::from_sample_rate_hz(16000),
        i2s::config::StdSlotConfig::philips_slot_default(i2s::config::DataBitWidth::Bits16, i2s::config::SlotMode::Stereo),
        i2s::config::StdGpioConfig::default(),
    );
    
    let mut i2s_rx = i2s::I2sDriver::new_std_rx(
        peripherals.i2s0,
        &i2s_config,
        pins.gpio17, // BCLK
        pins.gpio45, // WS
        Some(pins.gpio2), // MCLK
        pins.gpio16, // DIN (Mic)
    )?;
    i2s_rx.rx_enable()?;

    // --- 5. Starting Worker Threads ---
    // IMU Thread (Consumes I2C0)
    let imu_dev = Icm42670::new(i2c0, Address::Primary).expect("IMU Init");
    thread::spawn(move || imu_thread(imu_dev));
 
    // Dock Sensor Thread (Consumes I2C1)
    thread::spawn(move || sensor_thread(i2c1));
 
    // TCP Metrics & Command Thread
    thread::spawn(move || tcp_thread());
 
    // Weather Fetch Thread
    thread::spawn(move || weather_thread());

    // Audio Thread — pinned to Core 1 to isolate from UI on Core 0
    ThreadSpawnConfiguration {
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core1),
        ..Default::default()
    }.set().unwrap();
    thread::spawn(move || audio_thread(i2s_rx));
    // Reset thread config to default for any future spawns
    ThreadSpawnConfiguration::default().set().unwrap();

    // --- 6. Dashboard UI Loop (Double-Buffered) ---
    info!("All worker threads started. Entering main loop...");
    let mut fb = FrameBuffer::new(320, 240);

    const CST_OFFSET: u64 = 8 * 3600; // UTC+8 China Standard Time

    loop {
        // Update unix_time from system clock
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now > 1_000_000_000 {
            if let Ok(mut status) = get_status().write() {
                status.unix_time = now + CST_OFFSET;
            }
        }

        // Render to framebuffer (in-memory, instant)
        fb.buf.fill(0); // Clear to black
        if let Ok(status) = get_status().read() {
            let _ = draw_dashboard(&mut fb, &*status);
        }

        // Flush entire frame to display in one DMA transfer
        let _ = flush_framebuffer(&mut display, &fb);

        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
