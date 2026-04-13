//! ESP32-S3-BOX-3 Cyber-Hub Firmware (Standard Edition)
//! Architecture: Decoupled Multi-threaded IO with Synchronous Audio Pipeline

use esp_idf_hal::delay::Ets;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::i2c;
use esp_idf_svc::hal::i2s;
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sntp::EspSntp;
use esp_idf_svc::sntp::SyncStatus;
use log::*;
use mipidsi::options::{Orientation, Rotation};
use std::thread;
use std::time::SystemTime;

use cyber_hub_std::audio::{audio_thread, CodecConfig};
pub use cyber_hub_std::get_status;
use cyber_hub_std::imu::imu_thread;
use cyber_hub_std::sensor::sensor_thread;
use cyber_hub_std::tcp::tcp_thread;
use cyber_hub_std::weather::weather_thread;
use cyber_hub_std::wifi::connect_wifi;

use cyber_hub_std::display::{draw_cyber_hub_ui, draw_dashboard, draw_voice_screen, flush_framebuffer, FrameBuffer};

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
    let i2c1_driver = i2c::I2cDriver::new(
        peripherals.i2c1,
        pins.gpio41,
        pins.gpio40,
        &i2c::I2cConfig::new().baudrate(100_u32.kHz().into()),
    )?;
    let i2c1_shared = std::sync::Arc::new(std::sync::Mutex::new(i2c1_driver));

    // --- 3. Display ---
    let spi = spi::SpiDriver::new(
        peripherals.spi2,
        pins.gpio7,             // SCK
        pins.gpio6,             // MOSI
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
    rst.set_high()?;
    Ets::delay_ms(20);
    rst.set_low()?;
    Ets::delay_ms(150);

    let mut display = mipidsi::Builder::new(mipidsi::models::ILI9342CRgb565, di)
        .orientation(Orientation::new().rotate(Rotation::Deg180))
        .color_order(mipidsi::options::ColorOrder::Bgr)
        .init(&mut esp_idf_svc::hal::delay::Ets)
        .map_err(|_| anyhow::anyhow!("Display Init Fail"))?;
    Ets::delay_ms(100);
    let mut backlight = gpio::PinDriver::output(pins.gpio47)?;
    backlight.set_high()?;

    let mut pa_ctrl = gpio::PinDriver::output(pins.gpio46)?;
    pa_ctrl.set_high()?;

    // --- 2. WiFi Initialization ---
    draw_cyber_hub_ui(&mut display, "Connecting to WiFi...").expect("Draw UI");
    let _wifi = connect_wifi(peripherals.modem, sysloop.clone(), nvs.clone())?;

    draw_cyber_hub_ui(&mut display, "WiFi Connected! \nSyncing Time...").expect("Draw UI");

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
    // --- 4. I2S Std Philips Stereo, 16-bit slots (avoids Mono BCLK halving; audio.rs extracts L ch) ---
    let slot_config = i2s::config::StdSlotConfig::philips_slot_default(
        i2s::config::DataBitWidth::Bits16,
        i2s::config::SlotMode::Stereo,
    )
    .slot_bit_width(i2s::config::SlotBitWidth::Bits16);
    let clk_config = i2s::config::StdClkConfig::from_sample_rate_hz(16000)
        .mclk_multiple(i2s::config::MclkMultiple::M256);
    let channel_config = i2s::config::Config::default()
        .auto_clear(true)
        .dma_buffer_count(6)
        .frames_per_buffer(512);
    let i2s_config = i2s::config::StdConfig::new(
        channel_config,
        clk_config,
        slot_config,
        i2s::config::StdGpioConfig::default(),
    );
    let mut i2s_driver = i2s::I2sDriver::new_std_bidir(
        peripherals.i2s0,
        &i2s_config,
        pins.gpio17,      // bclk
        pins.gpio16,      // din
        pins.gpio15,      // dout
        Some(pins.gpio2), // mclk
        pins.gpio45,      // ws
    )?;
    i2s_driver.tx_enable()?;
    i2s_driver.rx_enable()?;
    std::thread::sleep(std::time::Duration::from_millis(50));
    // --- 5. Worker Threads ---
    let i2c0_imu = i2c0_shared.clone();
    thread::spawn(move || imu_thread(i2c0_imu));

    let i2c1_sensor = i2c1_shared.clone();
    thread::spawn(move || sensor_thread(i2c1_sensor));

    thread::spawn(move || tcp_thread());
    thread::spawn(move || weather_thread());

    let codec_config = CodecConfig { i2c: i2c0_shared };
    // --- 9. Start Audio Engine (on Core 1 to avoid WDT starvation on Core 0) ---
    // Configuring ThreadSpawn for Audio specifically
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"audio-cluster\0").unwrap()),
        stack_size: 30 * 1024,
        priority: 20,
        pin_to_core: Some(esp_idf_hal::cpu::Core::Core0),
        ..Default::default()
    }
    .set()
    .ok();

    thread::spawn(move || {
        audio_thread(i2s_driver, codec_config);
    });

    // Reset spawn config for other threads
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default()
        .set()
        .ok();

    // --- 6. UI Loop ---
    let mut fb = FrameBuffer::new(320, 240);
    const CST_OFFSET: u64 = 8 * 3600;
    let status_ref = get_status();
    loop {
        let cur_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        {
            let mut s = status_ref.write().unwrap();
            s.unix_time = cur_time + CST_OFFSET;
        }
        // Render to framebuffer (in-memory, instant)
        fb.buf.fill(0); // Clear to black
        if let Ok(status) = status_ref.read() {
            if status.voice_state != 0 {
                let _ = draw_voice_screen(&mut fb, &*status);
            } else {
                let _ = draw_dashboard(&mut fb, &*status);
            }
        }
        let _ = flush_framebuffer(&mut display, &fb);
        let is_voice_active = status_ref.read().map_or(false, |s| s.voice_state != 0);
        thread::sleep(std::time::Duration::from_millis(if is_voice_active { 50 } else { 200 }));
    }
}
