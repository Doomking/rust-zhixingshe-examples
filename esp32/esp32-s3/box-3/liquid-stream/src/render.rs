use embedded_graphics::{
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::*,
    primitives::Rectangle,
};
use log::info;
use std::time::Instant;

use crate::physics::FluidSim;

/// 全屏 565 缓冲（与 mipidsi fill_contiguous 一致）
pub struct FrameBuffer {
    pub buf: Vec<u16>,
    pub width: u32,
    pub height: u32,
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            buf: vec![0; (width * height) as usize],
            width,
            height,
        }
    }
}

pub struct RenderEngine {
    fps_counter: u32,
    last_time: Instant,
    fb: FrameBuffer,
    last_render_cost_ms: f32,
}

impl RenderEngine {
    pub fn new(_num_particles: usize) -> Self {
        Self {
            fps_counter: 0,
            last_time: Instant::now(),
            fb: FrameBuffer::new(320, 240),
            last_render_cost_ms: 0.0,
        }
    }

    pub fn last_render_cost_ms(&self) -> f32 {
        self.last_render_cost_ms
    }

    const OB: u16 = 0x10a2;

    #[inline]
    fn plot_max(fb: &mut [u16], w: i32, x: i32, y: i32, c: u16) {
        let h = fb.len() as i32 / w;
        if x < 0 || x >= w || y < 0 || y >= h {
            return;
        }
        let o = (y * w + x) as usize;
        if fb[o] == Self::OB {
            return;
        }
        if fb[o] < c {
            fb[o] = c;
        }
    }

    /// 参考 assets/output.gif：**每个粒子**是屏上独立亮青点（亚像素抖动、不按 10px 格对齐），
    /// 绝不把整格涂成俄罗斯方块；与「粒子不重叠」的物理斥力一致。
    pub fn render_fluid<D>(&mut self, display: &mut D, fluid: &FluidSim) -> anyhow::Result<()>
    where
        D: DrawTarget<Color = Rgb565>,
        D::Error: std::fmt::Debug,
    {
        let frame_start = Instant::now();
        let fb_w = self.fb.width as i32;
        let fb_h = self.fb.height as i32;
        let buf = &mut self.fb.buf;

        buf.fill(0x0000);

        let (nx, ny) = fluid.grid_dim();
        let cell = fluid.cell_size().round() as i32;
        // 含最外圈固体格，屏上能读到「容器」；否则粒子在隐形边界反弹，会像往虚空里落沙
        for gy in 0..ny {
            for gx in 0..nx {
                if !fluid.is_solid(gx, gy) {
                    continue;
                }
                let x0 = (gx as i32 * cell).max(0);
                let y0 = (gy as i32 * cell).max(0);
                let x1 = ((gx + 1) as i32 * cell).min(fb_w);
                let y1 = ((gy + 1) as i32 * cell).min(fb_h);
                for py in y0..y1 {
                    for px in x0..x1 {
                        if px >= 0 && px < fb_w && py >= 0 && py < fb_h {
                            let o = (py * fb_w + px) as usize;
                            if buf[o] < Self::OB {
                                buf[o] = Self::OB;
                            }
                        }
                    }
                }
            }
        }

        const C_WATER: u16 = 0x05de; // 统一的科技蓝水色

        for p in fluid.particles() {
            let px = p.position.x as i32;
            let py = p.position.y as i32;

            // 画半径为 4.0 的实心圆盘（直径 8 像素，完美覆盖 6.0 的物理排斥距离）
            // 因为互相交叠，视觉上会融合成一整块连续的液体表面
            for dy in -4..=4 {
                for dx in -4..=4 {
                    if dx * dx + dy * dy <= 16 {
                        Self::plot_max(buf, fb_w, px + dx, py + dy, C_WATER);
                    }
                }
            }
        }

        display
            .fill_contiguous(
                &Rectangle::new(Point::zero(), Size::new(self.fb.width, self.fb.height)),
                self.fb.buf.iter().map(|&raw| Rgb565::from(RawU16::new(raw))),
            )
            .map_err(|e| anyhow::anyhow!("DMA Flush Error: {:?}", e))?;

        self.last_render_cost_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        self.fps_counter += 1;
        if self.last_time.elapsed().as_secs() >= 1 {
            let actual_particles = fluid.particles().len();
            info!(
                "Render: {} FPS | particles={} | continuous blob | cost={:.2}ms",
                self.fps_counter,
                actual_particles,
                self.last_render_cost_ms
            );
            self.fps_counter = 0;
            self.last_time = Instant::now();
        }

        Ok(())
    }
}
