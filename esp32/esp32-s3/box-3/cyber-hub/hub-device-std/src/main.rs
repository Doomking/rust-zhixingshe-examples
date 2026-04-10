//! ESP32-S3-BOX-3 Cyber-Hub Firmware (Standard Edition)
//! Architecture: Decoupled Multi-threaded IO with Synchronous Audio Pipeline

use esp_idf_hal::delay::Ets;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::hal::i2c;
use esp_idf_svc::hal::i2s;
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::sntp::EspSntp;
use esp_idf_svc::sntp::SyncStatus;
use mipidsi::options::{Orientation, Rotation};
use log::*;
use std::thread;
use std::time::SystemTime;

use cyber_hub_std::audio::{audio_thread, CodecConfig};
use cyber_hub_std::imu::{imu_thread};
use cyber_hub_std::sensor::sensor_thread;
use cyber_hub_std::wifi::connect_wifi;
use cyber_hub_std::tcp::tcp_thread;
use cyber_hub_std::weather::weather_thread;
pub use cyber_hub_std::get_status;

use cyber_hub_std::display::{FrameBuffer, draw_dashboard, flush_framebuffer, draw_cyber_hub_ui};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;
    let sysloop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take()?;

    info!("Cyber-Hub Standard Edition - Booting...");


    // --- 1. WiFi Initialization ---
    // let _wifi = connect_wifi(peripherals.modem, sysloop, nvs)?;

    // --- 2. Shared I2C0 ---
    let i2c0_config = i2c::I2cConfig::new().baudrate(100_u32.kHz().into());
    let i2c0_driver = i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c0_config)?;
    let i2c0_shared = std::sync::Arc::new(std::sync::Mutex::new(i2c0_driver));
    let i2c1 = i2c::I2cDriver::new(peripherals.i2c1, pins.gpio41, pins.gpio40, &i2c::I2cConfig::new().baudrate(100_u32.kHz().into()))?;

    // --- 3. Display ---
    let spi = spi::SpiDriver::new(
        peripherals.spi2,
        pins.gpio7,      // SCK
        pins.gpio6,      // MOSI
        None::<gpio::AnyIOPin>, // MISO
        &spi::SpiDriverConfig::new(),
    )?;
    let spi_config = spi::config::Config::new().baudrate(26_u32.MHz().into());
    // let display_spi = spi::SpiDeviceDriver::new_single(
    //     peripherals.spi2, pins.gpio6, pins.gpio7, Option::<gpio::AnyIOPin>::None, Some(pins.gpio5),
    //     &spi::config::DriverConfig::default(), &spi_config,
    // )?;
   let spi_device = spi::SpiDeviceDriver::new(spi, Some(pins.gpio5), &spi_config)?; // CS = GPIO5 
    let dc = gpio::PinDriver::output(pins.gpio4)?;
    let di = display_interface_spi::SPIInterface::new(spi_device, dc);
    
    // --- Box-3 Display Reset (Fixed Polarity) ---
    let mut rst = gpio::PinDriver::output(pins.gpio48)?;
    // Hardware Reset
    rst.set_high()?; Ets::delay_ms(20); rst.set_low()?; Ets::delay_ms(150);

    let mut display = mipidsi::Builder::new(mipidsi::models::ILI9342CRgb565, di)
        .orientation(Orientation::new().rotate(Rotation::Deg180))
        .color_order(mipidsi::options::ColorOrder::Bgr)
        .init(&mut esp_idf_svc::hal::delay::Ets)
        .map_err(|_| anyhow::anyhow!("Display Init Fail"))?;
    Ets::delay_ms(100);
    let mut backlight = gpio::PinDriver::output(pins.gpio47)?;
    backlight.set_high()?;

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
    // --- 4. I2S ---
    // --- 4. I2S (Configured for 32-bit Slot Alignment) ---
    // Note: We use Bits32 to ensure the clock matches BOX-3 codec requirements.
    // Audio.rs will handle the conversion back to 16-bit for the AFE engine.
    let slot_config = i2s::config::StdSlotConfig::philips_slot_default(
        i2s::config::DataBitWidth::Bits32,
        i2s::config::SlotMode::Stereo
    );
    let i2s_config = i2s::config::StdConfig::new(i2s::config::Config::default(), i2s::config::StdClkConfig::from_sample_rate_hz(16000), slot_config, i2s::config::StdGpioConfig::default());
    
    let mut i2s_driver = i2s::I2sDriver::new_std_bidir(
        peripherals.i2s0, 
        &i2s_config, 
        pins.gpio17,        // bclk
        pins.gpio16,        // din (Capture)
        pins.gpio15,        // dout (Playback)
        Some(pins.gpio2),   // mclk
        pins.gpio45         // ws
    )?;
    i2s_driver.tx_enable()?;
    i2s_driver.rx_enable()?;

    // --- 5. Worker Threads ---
    let i2c0_imu = i2c0_shared.clone();
    thread::spawn(move || imu_thread(i2c0_imu));
    thread::spawn(move || sensor_thread(i2c1));
    thread::spawn(move || tcp_thread());
    thread::spawn(move || weather_thread());

    let codec_config = CodecConfig { i2c: i2c0_shared.clone() };
    // --- 9. Start Audio Engine (on Core 1 to avoid WDT starvation on Core 0) ---
    // Configuring ThreadSpawn for Audio specifically
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"audio-cluster\0").unwrap()),
        stack_size: 30 * 1024,
        priority: 20,
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core0),
        ..Default::default()
    }.set().ok();

    thread::spawn(move || {
        audio_thread(i2s_driver, codec_config);
    });
    
    // Reset spawn config for other threads
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default().set().ok();

    // --- 6. UI Loop ---
    let mut fb = FrameBuffer::new(320, 240);
    const CST_OFFSET: u64 = 8 * 3600;
    let status_ref = get_status();
    loop {
        let cur_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        {
            let mut s = status_ref.write().unwrap();
            s.unix_time = cur_time + CST_OFFSET;
        }
        let status = status_ref.read().unwrap().clone();
        // Render to framebuffer (in-memory, instant)
        fb.buf.fill(0); // Clear to black
        if let Ok(status) = get_status().read() {
            let _ = draw_dashboard(&mut fb, &*status);
        }
        let _ = flush_framebuffer(&mut display, &fb);
        thread::sleep(std::time::Duration::from_millis(200));
    }
}
