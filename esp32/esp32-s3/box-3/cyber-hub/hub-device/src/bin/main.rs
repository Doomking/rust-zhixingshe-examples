#![no_std]
// 不使用 Rust 标准库 (std)，因为 ESP32 属于嵌入式裸机环境，没有完整的操作系统支持
#![no_main] // 禁用默认的 main 函数，我们会使用 `#[esp_rtos::main]` 宏来定义入口
#![deny(
    clippy::mem_forget,
    reason = "在 esp_hal 中，内存泄漏（mem::forget）是非常危险的，尤其是涉及硬件 DMA 缓冲区的场景。"
)]
#![deny(clippy::large_stack_frames)]

use defmt::info; // Defmt 是一个专门为嵌入式系统设计的极度轻量级日志框架
use embassy_executor::Spawner; // Embassy 异步执行器的任务生成器
use embassy_net::{Config, StackResources}; // Embassy 网络协议栈配置和核心栈对象
use embassy_time::{Duration, Timer}; // Embassy 的异步定时器依赖
use esp_hal::clock::CpuClock; // ESP-HAL 时钟配置
use esp_hal::rng::Rng; // 硬件随机数发生器
use esp_hal::timer::timg::TimerGroup; // 硬件定时器外设，用于驱动系统的异步时钟

// 从外部模块引入刚刚重构好的三大网络任务
use cyber_hub::audio::{Es8311, audio_record_task, dummy_tx_task};
use cyber_hub::display::draw_cyber_hub_ui;
use cyber_hub::imu::imu_task;
use cyber_hub::tcp::tcp_client_task;
use cyber_hub::wifi::{net_task, wifi_task};

use esp_backtrace as _; // 引入官方的无敌堆栈回溯与 Panic 处理工具

// 引入全局内存分配器（因为 no_std 没有 std，所以需要手动引入 alloc 库才能使用 Box/String/Vec）
extern crate alloc;

// 这句宏会生成 esp-idf bootloader 所需的应用描述符，是程序的固件信息头
esp_bootloader_esp_idf::esp_app_desc!();

// 简单的宏：为了在 no_std 中避免使用生命周期到处乱飞，我们将数据放到静态堆空间
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

// (原有的网络与 Wi-Fi 任务已经被移动到了 src/wifi.rs 和 src/tcp.rs)

// ------------------------------------------------------------------------------------------------ //
// ✈️ 飞行起点：主函数入口
// ------------------------------------------------------------------------------------------------ //
#[allow(clippy::large_stack_frames)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // 1. 初始化 RTT (Real-Time Transfer) 和 Defmt 提供极速的硬件级终端调试输出
    rtt_target::rtt_init_defmt!();

    // 2. 初始化硬件配置，并将 CPU 时钟拉到最高性能档位
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config); // 获取所有的 ESP 硬件外设控制权（比如 SPI、I2C 等）

    // 3. 初始化全局环境中的堆内存（这允许代码能够像在电脑上一样使用 String 和 Vec）
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    // 4. 初始化硬件定时器 (TimerGroup) 以便支撑由 Embassy 维护的 Async/Await 的基于时间的任务调度
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    info!("Embassy initialized!");

    // ------------------------------------------------------------------- //
    // 网络与射频设备初始化
    // ------------------------------------------------------------------- //
    // ESPRadio 负责控制底层无线射频硬件
    let radio_init = alloc::boxed::Box::leak(alloc::boxed::Box::new(
        esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller"),
    ));

    // 生成 Wi-Fi Controller (用于控制连接逻辑) 和 Interface (用于绑定 TCP/IP 协议栈)
    let (wifi_controller, interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta; // 拿到客户端 (Station) 的虚拟网卡

    #[allow(unused_mut)]
    let mut rng = Rng::new(); // 生成随机数种子（对于 TCP 连接安全和 DHCP 握手很重要）
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // 选择 DHCP 协议（自动获取 IP 地址）
    let net_config = Config::dhcpv4(Default::default());

    // Embassy Net 协议栈的大心脏，它接管了刚刚获取的底层网卡(Interface)
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        net_config,
        mk_static!(StackResources<3>, StackResources::<3>::new()), // 规定这个栈里最多开 3 个 Socket
        seed,
    );

    // ------------------------------------------------------------------- //
    // SPI 屏幕物理驱动初始化 (Phase 2)
    // ------------------------------------------------------------------- //
    use esp_hal::gpio::{Level, Output, OutputConfig};
    use esp_hal::spi::master::Spi;

    let sck = peripherals.GPIO7;
    let mosi = peripherals.GPIO6;
    let dc = Output::new(peripherals.GPIO4, Level::Low, OutputConfig::default());
    let cs = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default()); // 软件 CS，保证整块传输期间 CS 不抖动
    // ILI9342C on BOX-3: reset_active_high=1 - we manage reset manually
    let mut rst = Output::new(peripherals.GPIO48, Level::High, OutputConfig::default());
    let mut backlight = Output::new(peripherals.GPIO47, Level::Low, OutputConfig::default());

    // 👇 【只需要加上这极其致命的一行代码！】👇
    // 强行拉高 PA_CTRL (GPIO46)，为底层的 Mute 缓冲芯片和音频放大器接通物理电源！
    let _pa_ctrl = Output::new(peripherals.GPIO46, Level::High, OutputConfig::default());

    info!("Initializing SPI Display...");
    let spi_config = esp_hal::spi::master::Config::default()
        .with_frequency(esp_hal::time::Rate::from_mhz(40))
        .with_mode(esp_hal::spi::Mode::_0);

    let spi = Spi::new(peripherals.SPI2, spi_config)
        .expect("SPI Init")
        .with_sck(sck)
        .with_mosi(mosi);

    // 自定义了一个带 FIFO 分块保护的 SPI 包装器
    struct ChunkedSpiBus<B>(B);
    impl<B: embedded_hal::spi::ErrorType> embedded_hal::spi::ErrorType for ChunkedSpiBus<B> {
        type Error = B::Error;
    }
    impl<B: embedded_hal::spi::SpiBus<u8>> embedded_hal::spi::SpiBus<u8> for ChunkedSpiBus<B> {
        fn read(&mut self, words: &mut [u8]) -> Result<(), B::Error> {
            self.0.read(words)
        }
        fn write(&mut self, words: &[u8]) -> Result<(), B::Error> {
            for chunk in words.chunks(60) {
                self.0.write(chunk)?;
            }
            Ok(())
        }
        fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), B::Error> {
            self.0.transfer(read, write)
        }
        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), B::Error> {
            self.0.transfer_in_place(words)
        }
        fn flush(&mut self) -> Result<(), B::Error> {
            self.0.flush()
        }
    }

    let spi_device =
        embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(ChunkedSpiBus(spi), cs).expect("SPI");
    let di = display_interface_spi::SPIInterface::new(spi_device, dc);

    let mut delay = esp_hal::delay::Delay::new();

    // 手动执行 Active-HIGH Reset序列: HIGH(复位) -> 延时 -> LOW(释放)
    rst.set_high(); // 触发复位
    delay.delay_millis(20u32);
    rst.set_low(); // 释放复位，进入正常工作模式
    delay.delay_millis(150u32); // 等待 ILI9342C 内部初始化完成

    info!("Initializing ILI9342C SPI Display...");
    let builder = mipidsi::Builder::new(mipidsi::models::ILI9342CRgb565, di).orientation(
        mipidsi::options::Orientation::new().rotate(mipidsi::options::Rotation::Deg180),
    );
    // 注意：不使用 .reset_pin()，因为 mipidsi 假设 active-LOW reset，对 BOX-3 是反的

    info!("Builder configured. Calling init()...");
    let mut display = match builder.init(&mut delay) {
        Ok(d) => {
            info!("Mipidsi display init successful!");
            d
        }
        Err(_e) => {
            defmt::error!("FATAL: Display init failed with error!");
            panic!("Display init error");
        }
    };

    // 点亮背光
    backlight.set_high();

    // 正式 UI
    draw_cyber_hub_ui(&mut display, "INIT: Wait for DHCP...").expect("Failed to draw CyberHub UI");

    // ------------------------------------------------------------------- //
    // I2S & DMA 手动分配 (Fix Phase 3 Panic: OutOfDescriptors)
    // ------------------------------------------------------------------- //
    use esp_hal::i2s::master::{Config as I2sConfig, DataFormat, I2s}; // 确保顶部引入了这个

    let dma_channel = peripherals.DMA_CH0;
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
        esp_hal::dma_circular_buffers!(32768, 4096);

    // 启动 I2S 接口 (录音: GPIO16, 播音: GPIO15, 时钟: GPIO2/17/45)
    let i2s = I2s::new(
        peripherals.I2S0,
        dma_channel,
        I2sConfig::default()
            .with_data_format(DataFormat::Data16Channel16)
            .with_sample_rate(esp_hal::time::Rate::from_hz(16000)),
    )
    .expect("I2S Init Failed")
    .into_async()
    .with_mclk(peripherals.GPIO2);

    let i2s_rx = i2s
        .i2s_rx
        .with_din(peripherals.GPIO16)
        .build(rx_descriptors);

    let i2s_tx = i2s
        .i2s_tx
        .with_bclk(peripherals.GPIO17)
        .with_ws(peripherals.GPIO45)
        .with_dout(peripherals.GPIO15)
        .build(tx_descriptors);

    // ------------------------------------------------------------------- //
    // I2C & 音频/陀螺仪外设初始化 (Phase 3)
    // ------------------------------------------------------------------- //
    use cyber_hub::audio::Es7210;
    use esp_hal::i2c::master::I2c;
    use icm42670::{Address, Icm42670};

    info!("Initializing I2C Bus...");
    let mut i2c = I2c::new(peripherals.I2C0, esp_hal::i2c::master::Config::default())
        .expect("I2C Init")
        .with_sda(peripherals.GPIO8)
        .with_scl(peripherals.GPIO18);

    // ------------------------------------------------------------------- //
    // 网络任务派发 (Spawning)
    // ------------------------------------------------------------------- //
    // 必须首先派发网络任务！因为 WiFi 底层连接时会极大幅度地阻塞 CPU(长达1-2秒)
    spawner.spawn(net_task(runner)).unwrap();
    spawner.spawn(wifi_task(wifi_controller)).unwrap();
    spawner.spawn(tcp_client_task(stack)).unwrap();

    info!("Waiting 3.5 seconds for WiFi CPU-hogging to finish before arming Audio DMA...");
    Timer::after(Duration::from_millis(3500)).await;

    // ------------------------------------------------------------------- //
    // 硬件初始化与启动 (此时 CPU 已经完全空闲，没有任何阻塞)
    // ------------------------------------------------------------------- //
    // 1. 启动音频硬件引擎 (先开 I2S DMA 时钟！保证引脚立刻输出 BCLK 和 LRCK)
    spawner.spawn(dummy_tx_task(i2s_tx, tx_buffer)).unwrap();
    spawner.spawn(audio_record_task(i2s_rx, rx_buffer)).unwrap();
    
    // 短暂等待 DMA 泵稳定时钟信号 (50毫秒足够)
    Timer::after(Duration::from_millis(50)).await;

    // 2. 初始化音频编解码器 ES8311 (扬声器)
    let mut codec_out = Es8311::new(&mut i2c);
    codec_out.init().await.expect("ES8311 Init");

    // 3. 初始化音频编解码器 ES7210 (双麦克风阵列)
    // 此时 BCLK/WS 已经在稳定输出，Reset将完美锁相！
    let mut codec_in = Es7210::new(&mut i2c);
    codec_in.init().await.expect("ES7210 Init");

    // 4. 将 I2C 移交给陀螺仪驱动 (ICM42607-P)
    let imu = Icm42670::new(i2c, Address::Primary).expect("Failed to init IMU");
    spawner.spawn(imu_task(imu)).unwrap();

    info!("All systems GO! Device completely armed.");

    // 主事件循环中，我们观察一下是否获取到了 DHCP IP 地址
    loop {
        if stack.is_config_up() {
            if let Some(config) = stack.config_v4() {
                info!("IP address: {:?}", defmt::Debug2Format(&config.address));
                draw_cyber_hub_ui(&mut display, "STATUS: TCP CONNECTED").ok();
                break;
            }
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    // 后续主循环就可以放开手脚做别的事了，比如更新屏幕、读传感器等
    loop {
        info!("Hello world! We are connected to WiFi.");
        Timer::after(Duration::from_secs(5)).await; // 每隔 5 秒运行一次休眠
    }
}
