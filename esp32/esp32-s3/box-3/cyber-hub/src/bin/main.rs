#![no_std] // 不使用 Rust 标准库 (std)，因为 ESP32 属于嵌入式裸机环境，没有完整的操作系统支持
#![no_main] // 禁用默认的 main 函数，我们会使用 `#[esp_rtos::main]` 宏来定义入口
#![deny(
    clippy::mem_forget,
    reason = "在 esp_hal 中，内存泄漏（mem::forget）是非常危险的，尤其是涉及硬件 DMA 缓冲区的场景。"
)]
#![deny(clippy::large_stack_frames)]

use core::str::FromStr;
use defmt::{error, info, warn};           // Defmt 是一个专门为嵌入式系统设计的极度轻量级日志框架
use embassy_executor::Spawner;            // Embassy 异步执行器的任务生成器
use embassy_net::tcp::TcpSocket;          // Embassy 的 TCP Socket 实现
use embassy_net::{Config, Stack};         // Embassy 网络协议栈配置和核心栈对象
use embassy_net::StackResources;          // 预分配给网络栈的内存资源（存放 Socket 等）
use embassy_time::{Duration, Timer};      // Embassy 的异步定时器依赖
use esp_hal::clock::CpuClock;             // ESP-HAL 时钟配置
use esp_hal::rng::Rng;                    // 硬件随机数发生器
use esp_hal::timer::timg::TimerGroup;     // 硬件定时器外设，用于驱动系统的异步时钟
use esp_radio::wifi::{ClientConfig, ModeConfig, WifiController, WifiEvent, WifiDevice}; // 控制 Wi-Fi 硬件的模块

// 恐慌处理 (Panic Handler)：当程序发生严重错误崩溃时，调用此函数
// 在嵌入式里通常是写死一个死循环，或者重启设备
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

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

// ------------------------------------------------------------------------------------------------ //
// [后台任务 1]: 管理 Embassy 的底层协议栈
// 这是一个独立的异步任务，不断推进 TCP/IP 状态机的数据收发
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 2]: TCP 客户端任务，负责和 Mac 上的 Server 建立 TCP 连接
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
async fn tcp_client_task(stack: Stack<'static>) {
    // 预先给 Socket 分配 RX 和 TX 的缓冲区（内存）
    let mut rx_buffer = [0; 1024];
    let mut tx_buffer = [0; 1024];

    loop {
        // 如果物理链路没连上，或者 DHCP 还没获取到 IP，就先等待 500ms
        if !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        // 创建 TCP Socket 并绑定到当前的协议栈
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10))); // 设置超市时间 10秒

        // 将环境变量编译时传进来的 MAC_IP 解析为真正的 Ipv4 地址结构
        let mac_ip = embassy_net::Ipv4Address::from_str(env!("MAC_IP")).expect("Invalid MAC_IP");
        let endpoint = embassy_net::IpEndpoint::new(embassy_net::IpAddress::Ipv4(mac_ip), 8080);
        
        info!("Connecting to TCP server at {}...", env!("MAC_IP"));
        
        // 发起异步连接（这行代码会挂起让出 CPU，直到网络连接成功或者失败）
        if let Err(e) = socket.connect(endpoint).await {
            warn!("TCP connect error: {:?}", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(2)).await; // 连接失败就等 2 秒后重启大循环重试
            continue;
        }

        info!("TCP connected to Mac!");
        
        // 连上之后，往 Mac 发送一条 Hello Mac 数据
        let msg = b"Hello Mac from Cyber-Hub!";
        if let Err(e) = socket.write(msg).await {
            warn!("TCP write error: {:?}", defmt::Debug2Format(&e));
        } else {
            info!("Sent Hello Mac!");
        }

        // 发送完毕可以主动关闭 socket，也可以进入自己的收发循环。这里因为是测试，发完我们就关闭。
        socket.close();
        Timer::after(Duration::from_secs(5)).await;
    }
}

// ------------------------------------------------------------------------------------------------ //
// [后台任务 3]: Wi-Fi 状态管理机任务，专门负责连上路由器，并在断线时自动重连
// ------------------------------------------------------------------------------------------------ //
#[embassy_executor::task]
async fn wifi_task(mut controller: WifiController<'static>) {
    info!("Starting wifi task...");
    
    // 初始化客户端的配置（账号密码）
    // 这里用 env!() 宏读取编译时传入的环境变量，避免把密码明文写在代码中
    let client_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(alloc::string::String::from(env!("WIFI_SSID")))
            .with_password(alloc::string::String::from(env!("WIFI_PASS")))
    );
    
    // 下发配置到硬件
    controller.set_config(&client_config).expect("Failed to set configuration");

    // 启动 Wi-Fi 硬件
    match controller.start_async().await {
        Ok(_) => info!("Wifi started!"),
        Err(e) => {
            error!("Failed to start wifi: {:?}", defmt::Debug2Format(&e));
            return;
        }
    }

    loop {
        info!("Connecting to WiFi...");
        // 尝试连接并挂起等待结果
        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect: {:?}", defmt::Debug2Format(&e));
                Timer::after(Duration::from_millis(5000)).await;
                continue; // 重新大循环，也就是每 5 秒重试连接一遍
            }
        }

        // 挂起等待硬件抛出 [WifiEvent::StaDisconnected] 事件 (一旦抛出说明掉线了)
        controller.wait_for_event(WifiEvent::StaDisconnected).await;
        info!("WiFi disconnected. Reconnecting...");
        // 循环继续，自动走上一步的断线重连逻辑
    }
}

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
        esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller")
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
    // 任务派发 (Spawning)
    // ------------------------------------------------------------------- //
    // 我们将把写好的各个独立子系统变成并发的后台任务全部“扔”给调度系统运行。
    spawner.spawn(net_task(runner)).unwrap();
    spawner.spawn(wifi_task(wifi_controller)).unwrap();
    spawner.spawn(tcp_client_task(stack)).unwrap();

    info!("Waiting for DHCP config...");
    
    // 主事件循环中，我们观察一下是否获取到了 DHCP IP 地址
    loop {
        if stack.is_config_up() {
            if let Some(config) = stack.config_v4() {
                info!("IP address: {:?}", defmt::Debug2Format(&config.address));
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
