mod network;
mod protocol;
mod display;
mod jpeg;
mod codec_es8311;
mod audio;
mod sync;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::i2c;
use esp_idf_svc::hal::i2s;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::{error, info, warn};
use crossbeam_channel::bounded;
use std::sync::{Arc, Mutex};

use esp_idf_hal::gpio;
use esp_idf_hal::spi;
use esp_idf_hal::units::FromValueType;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("LiquidCast ESP Client: Phase 4 Booting (video decode + double-buffer render + audio)...");

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let ssid = env!("WIFI_SSID");
    let password = env!("WIFI_PASS");
    let server_addr = env!("SERVER_ADDR");

    // 1. Initialize Display
    info!("Initializing Display...");
    let spi = spi::SpiDriver::new(
        peripherals.spi2,
        pins.gpio7,             // SCK
        pins.gpio6,             // MOSI
        None::<gpio::AnyIOPin>, // MISO
        &spi::SpiDriverConfig::new().dma(spi::Dma::Auto(32768)),
    )?;
    
    let spi_config = spi::config::Config::new().baudrate(40_u32.MHz().into());
    let spi_device = spi::SpiDeviceDriver::new(spi, Some(pins.gpio5), &spi_config)?; // CS = GPIO5
    let dc = gpio::PinDriver::output(pins.gpio4)?;
    let rst = gpio::PinDriver::output(pins.gpio48)?;
    let backlight = gpio::PinDriver::output(pins.gpio47)?;

    let mut display_mgr = display::DisplayManager::new(spi_device, dc, rst, backlight)?;
    info!("Display path: liquid-stream compatible raw RAMWR DMA");

    // 2. Initialize Wi-Fi
    info!("Connecting to Wi-Fi...");
    let _network_manager =
        network::NetworkManager::connect_wifi(peripherals.modem, sysloop, nvs, ssid, password)?;

    // 3. BOX-3 codec bus + PA + I2S (ES8311 DAC, same pinout as Espressif BSP / cyber-hub)
    let i2c_cfg = i2c::I2cConfig::new().baudrate(100_u32.kHz().into());
    let i2c_driver = i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c_cfg)?;
    let i2c_shared = Arc::new(Mutex::new(i2c_driver));

    let mut pa_ctrl = gpio::PinDriver::output(pins.gpio46)?;
    pa_ctrl.set_high()?;

    let i2s_cfg = audio::box3_i2s_std_config();
    let mut i2s_driver = i2s::I2sDriver::new_std_bidir(
        peripherals.i2s0,
        &i2s_cfg,
        pins.gpio17,      // BCLK
        pins.gpio16,      // DIN
        pins.gpio15,      // DOUT
        Some(pins.gpio2), // MCLK
        pins.gpio45,      // WS
    )?;
    i2s_driver.tx_enable()?;
    i2s_driver.rx_enable()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 4. Channels + threads: TCP demux → video (decode) / audio (I2S)
    let (video_tx, video_rx) = bounded::<network::VideoPacket>(2);
    let (audio_tx, audio_rx) = bounded::<network::AudioPacket>(48);

    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"audio-i2s\0").unwrap()),
        stack_size: 24 * 1024,
        priority: 18,
        ..Default::default()
    }
    .set()
    .ok();

    let i2c_for_audio = i2c_shared.clone();
    std::thread::spawn(move || {
        audio::run_playback_thread(i2s_driver, i2c_for_audio, audio_rx);
    });
    drop(i2c_shared);

    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default().set().ok();

    let server_addr_owned = server_addr.to_string();
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration {
        name: Some(core::ffi::CStr::from_bytes_with_nul(b"tcp-client\0").unwrap()),
        stack_size: 16 * 1024,
        priority: 15,
        ..Default::default()
    }.set().ok();

    std::thread::spawn(move || {
        loop {
            if let Err(e) = network::start_tcp_client(
                &server_addr_owned,
                video_tx.clone(),
                audio_tx.clone(),
            ) {
                error!("TCP Client Error: {}. Retrying in 5 seconds...", e);
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });

    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default().set().ok();

    // 5. Decode and Render Loop (Main Thread)
    info!("Entering rendering loop...");
    let mut av_obs_drops: u32 = 0;
    let mut av_obs_count: u32 = 0;
    let mut av_obs_delta_sum: i64 = 0;
    let mut av_obs_last = std::time::Instant::now();

    loop {
        if let Ok(video_pkt) = video_rx.recv() {
            let audio_ms = audio::current_audio_time_ms();
            if audio_ms != 0 {
                let av_delta = (video_pkt.timestamp_ms as i32) - (audio_ms as i32);
                av_obs_count = av_obs_count.saturating_add(1);
                av_obs_delta_sum = av_obs_delta_sum.saturating_add(av_delta as i64);
                if av_delta < -sync::drop_late_ms() {
                    av_obs_drops = av_obs_drops.saturating_add(1);
                    warn!(
                        "AV sync: drop late video frame ts={} audio={} delta={}ms",
                        video_pkt.timestamp_ms, audio_ms, av_delta
                    );
                    continue;
                }
                if av_delta > sync::wait_ahead_ms() {
                    std::thread::sleep(std::time::Duration::from_millis(
                        (av_delta.min(80)) as u64,
                    ));
                }
            }

            let start_decode = std::time::Instant::now();
            
            match jpeg::decode_rgb565(&video_pkt.payload) {
                Ok(frame) => {
                    if frame.width > 320 || frame.height > 240 {
                        warn!("Image too large: {}x{}", frame.width, frame.height);
                        continue;
                    }
                    let non_zero = frame.rgb565_be.iter().take(2048).filter(|&&p| p != 0).count();

                    let decode_time = start_decode.elapsed();
                    let start_draw = std::time::Instant::now();
                    if let Err(e) =
                        display_mgr.draw_rgb565_be_pixels(frame.width, frame.height, &frame.rgb565_be)
                    {
                        error!("Display draw error: {:?}", e);
                        continue;
                    }
                    let draw_time = start_draw.elapsed();
                    info!(
                        "Frame OK: {}x{} Decode: {}ms | Draw: {}ms | nz(first2k)={}",
                        frame.width,
                        frame.height,
                        decode_time.as_millis(),
                        draw_time.as_millis(),
                        non_zero
                    );
                }
                Err(e) => {
                    error!("JPEG Decode error: {:?}", e);
                }
            }
        }

        if av_obs_last.elapsed().as_secs() >= 2 {
            if av_obs_count > 0 {
                let avg = av_obs_delta_sum / (av_obs_count as i64);
                info!(
                    "AV metrics 2s: samples={} drops={} drop_ratio={:.2}% avg_delta={}ms thresholds(drop_late={}, wait_ahead={})",
                    av_obs_count,
                    av_obs_drops,
                    (av_obs_drops as f32) * 100.0 / (av_obs_count as f32),
                    avg,
                    sync::drop_late_ms(),
                    sync::wait_ahead_ms(),
                );
            }
            av_obs_count = 0;
            av_obs_drops = 0;
            av_obs_delta_sum = 0;
            av_obs_last = std::time::Instant::now();
        }
        
        // Feed the FreeRTOS watchdog
        esp_idf_svc::hal::delay::FreeRtos::delay_ms(1);
    }
}
