mod network;
mod protocol;
mod display;
mod jpeg;
mod codec_es8311;
mod audio;
mod sync;
mod traits;

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
use traits::VideoOutput;

fn main() -> anyhow::Result<()> {
    // 初始化 ESP-IDF 系统补丁
    esp_idf_svc::sys::link_patches();
    // 初始化日志系统
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("LiquidCast ESP 客户端启动中 (视频解码 + 双缓冲渲染 + 音频播放)...");

    // 获取硬件外设
    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // 从编译时环境变量获取 Wi-Fi 凭据和服务器地址
    let ssid = env!("WIFI_SSID");
    let password = env!("WIFI_PASS");
    let server_addr = env!("SERVER_ADDR");

    // 1. 初始化显示屏
    info!("正在初始化显示屏...");
    let spi = spi::SpiDriver::new(
        peripherals.spi2,
        pins.gpio7,             // SCK (时钟线)
        pins.gpio6,             // MOSI (数据输出)
        None::<gpio::AnyIOPin>, // MISO (不需要)
        &spi::SpiDriverConfig::new().dma(spi::Dma::Auto(32768)), // 启用 DMA 以提高传输速度
    )?;
    
    let spi_config = spi::config::Config::new().baudrate(40_u32.MHz().into());
    let spi_device = spi::SpiDeviceDriver::new(spi, Some(pins.gpio5), &spi_config)?; // CS (片选线) = GPIO5
    let dc = gpio::PinDriver::output(pins.gpio4)?; // DC (数据/命令选择)
    let rst = gpio::PinDriver::output(pins.gpio48)?; // RST (重置)
    let backlight = gpio::PinDriver::output(pins.gpio47)?; // BL (背光)

    // 创建显示管理器
    let display_mgr = display::DisplayManager::new(spi_device, dc, rst, backlight)?;
    info!("显示屏初始化完成 (使用原始 RAMWR DMA 路径)");

    // 2. 初始化并连接 Wi-Fi
    info!("正在连接 Wi-Fi...");
    let _network_manager =
        network::NetworkManager::connect_wifi(peripherals.modem, sysloop, nvs, ssid, password)?;

    // 3. 初始化音频 (BOX-3 编解码器总线 + 功放 + I2S)
    let i2c_cfg = i2c::I2cConfig::new().baudrate(100_u32.kHz().into());
    let i2c_driver = i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c_cfg)?;
    let i2c_shared = Arc::new(Mutex::new(i2c_driver));

    // 开启功放控制引脚
    let mut pa_ctrl = gpio::PinDriver::output(pins.gpio46)?;
    pa_ctrl.set_high()?;

    // 配置 I2S 接口
    let i2s_cfg = audio::box3_i2s_std_config();
    let mut i2s_driver = i2s::I2sDriver::new_std_bidir(
        peripherals.i2s0,
        &i2s_cfg,
        pins.gpio17,      // BCLK (位时钟)
        pins.gpio16,      // DIN (输入)
        pins.gpio15,      // DOUT (输出)
        Some(pins.gpio2), // MCLK (主时钟)
        pins.gpio45,      // WS (左右声道切换时钟)
    )?;
    i2s_driver.tx_enable()?;
    i2s_driver.rx_enable()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 4. 创建线程间通信通道: TCP 解复用线程 -> 视频解码线程 / 音频播放线程
    let (video_tx, video_rx) = bounded::<network::VideoPacket>(2); // 视频缓冲较小，降低延迟
    let (audio_tx, audio_rx) = bounded::<network::AudioPacket>(128); // 音频缓冲较大，防止卡顿

    // 启动音频播放线程 (高优先级)
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

    // 恢复默认线程配置
    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default().set().ok();

    // 启动 TCP 客户端线程 (中等优先级)
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
                error!("TCP 客户端错误: {}。5秒后重试...", e);
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });

    esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration::default().set().ok();

    // 5. 视频解码与渲染主循环 (主线程)
    info!("进入视频渲染主循环...");
    let mut av_obs_drops: u32 = 0; // 丢帧统计
    let mut av_obs_count: u32 = 0; // 总帧数统计
    let mut av_obs_delta_sum: i64 = 0; // A/V 同步偏差累加
    let mut av_obs_last = std::time::Instant::now();

    // 视频预缓冲逻辑
    let mut video_ready = false;
    const VIDEO_PREBUF_TARGET: usize = 3; // 缓冲 3 帧后再开始播放
    let mut video_prebuf_cnt = 0;

    let mut display: Box<dyn VideoOutput> = Box::new(display_mgr);

    loop {
        if let Ok(video_pkt) = video_rx.recv() {
            // 处理预缓冲
            if !video_ready {
                video_prebuf_cnt += 1;
                if video_prebuf_cnt >= VIDEO_PREBUF_TARGET {
                    video_ready = true;
                    info!("视频预缓冲完成 ({} 帧)，开始渲染", video_prebuf_cnt);
                } else {
                    continue;
                }
            }

            // A/V 同步逻辑: 以音频时钟为基准
            let audio_ms = audio::current_audio_time_ms();
            if audio_ms != 0 {
                let av_delta = (video_pkt.timestamp_ms as i32) - (audio_ms as i32);
                av_obs_count = av_obs_count.saturating_add(1);
                av_obs_delta_sum = av_obs_delta_sum.saturating_add(av_delta as i64);

                // 如果视频太晚了，直接丢弃该帧以赶上进度
                if av_delta < -sync::drop_late_ms() {
                    av_obs_drops = av_obs_drops.saturating_add(1);
                    warn!(
                        "音画同步: 视频过晚，丢帧 ts={} audio={} delta={}ms",
                        video_pkt.timestamp_ms, audio_ms, av_delta
                    );
                    continue;
                }
                // 如果视频太快了，进行短暂休眠等待音频
                if av_delta > sync::wait_ahead_ms() {
                    std::thread::sleep(std::time::Duration::from_millis(
                        (av_delta.min(80)) as u64,
                    ));
                }
            }

            let start_decode = std::time::Instant::now();
            
            // 优化路径: 直接解码到显示器的后备缓冲区 (Zero-Copy)
            let backbuffer = display.get_backbuffer();
            match jpeg::decode_rgb565_to(&video_pkt.payload, backbuffer) {
                Ok(info) => {
                    if info.width > 320 || info.height > 240 {
                        warn!("图像超出屏幕尺寸: {}x{}", info.width, info.height);
                        continue;
                    }

                    let decode_time = start_decode.elapsed();
                    let start_draw = std::time::Instant::now();
                    
                    // 将缓冲区内容刷入显示屏
                    if let Err(e) = display.draw_from_backbuffer(info.width, info.height) {
                        error!("显示刷新错误: {:?}", e);
                        continue;
                    }
                    let draw_time = start_draw.elapsed();
                    
                    // 每 20 帧输出一次性能统计
                    if av_obs_count % 20 == 0 {
                        info!(
                            "帧渲染完成: {}x{} 解码: {}ms | 刷新: {}ms",
                            info.width, info.height,
                            decode_time.as_millis(), draw_time.as_millis()
                        );
                    }
                }
                Err(e) => {
                    error!("JPEG 解码失败: {:?}", e);
                }
            }
        }

        // 每 2 秒输出一次 A/V 同步统计信息
        if av_obs_last.elapsed().as_secs() >= 2 {
            if av_obs_count > 0 {
                let avg = av_obs_delta_sum / (av_obs_count as i64);
                info!(
                    "同步监控 (2s): 样本={} 丢帧={} 丢帧率={:.2}% 平均偏差={}ms (阈值: 延迟={}ms, 超前={}ms)",
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
        
        // 喂狗，给 FreeRTOS 调度让出时间
        esp_idf_svc::hal::delay::FreeRtos::delay_ms(1);
    }
}
