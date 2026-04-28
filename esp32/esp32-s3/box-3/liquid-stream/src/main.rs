mod hal;
mod sensor;
mod physics;
mod render;

use esp_idf_sys as _;
use glam::Vec2;
use log::info;
use esp_idf_svc::hal::peripherals::Peripherals;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use sensor::InputSource; // 需要显式导入 trait 以便获得 gravity 方法
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("LiquidStream: Phase 3 Booting (Physics Engine)...");

    let peripherals = Peripherals::take()?;
    let mut hw = hal::init_hardware(peripherals)?;

    hw.display.clear(Rgb565::BLACK).map_err(|_| anyhow::anyhow!("Clear failed"))?;
    info!("Display initialized.");

    let imu_input = sensor::start_imu_thread(hw.i2c0);

    // --- 3. Fluid Physics Initialization ---
    // 优化：采用 1000 个大颗粒（间距 6.0），既能保证填满半杯水，又能让帧率起飞
    let num_particles = 1000;
    // BOX-3 在当前旋转配置下按 320x240 全屏仿真
    let mut fluid = physics::FluidSim::new(num_particles, 320.0, 240.0);
    let mut render_engine = render::RenderEngine::new(num_particles);
    let mut tick_counter: u32 = 0;
    let mut perf_log_timer = Instant::now();

    info!(
        "PBF+FLIP: Fluid simulation with continuous blob rendering; 320×240"
    );
    info!("Starting Core Simulation Loop...");

    loop {
        let frame_start = Instant::now();
        // 模拟坐标 Y 向下增大；IMU 的 (gx,gy) 里 gy 常与「屏上向上」同号（日志里常接近 -1），
        // 须取 -gy 才得到「指向屏下方」的加速度，否则会整体往上推形成顶/底两坨。
        // G：略提重力与 dt，让同样倾角下位移更明显
        let g = imu_input.current_gravity();
        let g_len = g.length();
        // 根据 IMU 获取真实重力映射
        let gravity = Vec2::new(g.x, -g.y) * 300.0;

        let physics_start = Instant::now();
        fluid.step(0.040, gravity);
        let physics_ms = physics_start.elapsed().as_secs_f32() * 1000.0;

        // 3. 渲染引擎读取状态并擦写显存
        render_engine.render_fluid(&mut hw.display, &fluid)?;
        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        tick_counter += 1;

        if perf_log_timer.elapsed().as_secs() >= 1 {
            // J：区分「IMU 输入小」和「场里没速度」
            let v_max = fluid.max_particle_speed();
            let (nx, ny) = fluid.grid_dim();
            let mut occupied = 0usize;
            let mut max_d = 0u8;
            for y in 0..ny {
                for x in 0..nx {
                    let d = fluid.grid_density(x, y);
                    if d > 0 {
                        occupied += 1;
                        max_d = max_d.max(d);
                    }
                }
            }
            info!(
                "Tune: |g|={:.3} g=({:+.3},{:+.3}) | grav=({:+.1},{:+.1}) max|v|={:6.1} | ticks/s={} | phys={:5.1}ms | rend={:5.1}ms | frame={:5.1}ms | occ={} dmax={}",
                g_len,
                g.x,
                g.y,
                gravity.x,
                gravity.y,
                v_max,
                tick_counter,
                physics_ms,
                render_engine.last_render_cost_ms(),
                frame_ms,
                occupied,
                max_d
            );
            tick_counter = 0;
            perf_log_timer = Instant::now();
        }
        
        // 适当休眠不仅能降低发热，最重要的是必须让出 CPU 给 FreeRTOS 的 IDLE 任务去重置看门狗 (Task Watchdog)
        // std::thread::sleep 如果卡在 tick 边缘可能会导致唤醒过快，直接换用 FreeRtos::delay_ms 彻底释放时间片
        // 0 易触发看门狗；1ms 比 2ms 帧率更跟手
        esp_idf_svc::hal::delay::FreeRtos::delay_ms(1);
    }
}
