use embedded_graphics::{
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::*,
    primitives::Rectangle,
};
use log::info;
use std::time::Instant;

use crate::physics::FluidSim;
use crate::hal::{AppSpi, AppDc};
use esp_idf_svc::hal::spi::SpiDeviceDriver;

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
    pub fn render_fluid(&mut self, spi: &mut AppSpi, dc: &mut AppDc, fluid: &FluidSim) -> anyhow::Result<()>
    {
        let frame_start = Instant::now();
        let fb_w = self.fb.width as i32;
        let fb_h = self.fb.height as i32;
        let buf = &mut self.fb.buf;

        buf.fill(0x0000);

        // 不再绘制物理底层默认的网格边界，避免在屏幕四周产生难看的紫色/灰色边框

        const C_WATER: u16 = 0xde05; // 0x05de.to_be() // 统一的科技蓝水色

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

        // --- 绘制全息玻璃容器边框 ---
        let box_x0 = 76;
        let box_y0 = 96;
        let box_x1 = 236;
        let box_y1 = 136;
        
        let c_glass: u16 = 0x2400; // 0x0024.to_be() // 极幽暗的深蓝色
        let c_border: u16 = 0xE003; // 0x03E0.to_be() // 暗青色边框
        let c_corner: u16 = 0xff07; // 0x07ff.to_be() // 高亮青色四角
        
        for py in box_y0..box_y1 {
            for px in box_x0..box_x1 {
                if px >= 0 && px < fb_w && py >= 0 && py < fb_h {
                    let o = (py * fb_w + px) as usize;
                    
                    let is_corner_x = (px - box_x0 < 8) || (box_x1 - px <= 8);
                    let is_corner_y = (py - box_y0 < 8) || (box_y1 - py <= 8);
                    let is_edge_x = px == box_x0 || px == box_x1 - 1;
                    let is_edge_y = py == box_y0 || py == box_y1 - 1;
                    
                    if (is_edge_x && is_corner_y) || (is_edge_y && is_corner_x) {
                        buf[o] = c_corner;
                    } else if is_edge_x || is_edge_y {
                        // buf[o] = c_border; // 或者干脆不画长边框，只画四角，更高级！
                        if py % 4 == 0 || px % 4 == 0 {
                            buf[o] = c_border; // 虚线边框
                        }
                    } else {
                        // 内部区域如果没被水覆盖，则画玻璃底色
                        if buf[o] == 0 {
                            buf[o] = c_glass;
                        }
                    }
                }
            }
        }

        // --- 绘制中心发光文字 "无限光河" 作为物理障碍的可视化 ---
        const C_NEON_CYAN: u16 = 0xff07; // 0x07ff.to_be() // 发光青色
        const TEXT_W: i32 = 16;
        const TEXT_H: i32 = 16;
        const CHARS: [[u16; 16]; 4] = [
            [0x3FFC, 0x0100, 0x0100, 0x0100, 0x0300, 0x7FFE, 0x0380, 0x0280, 0x0680, 0x0680, 0x0C82, 0x1882, 0x3086, 0x60FE, 0x0000, 0x0000],
            [0x7FFC, 0x6504, 0x6D04, 0x69FC, 0x7904, 0x6904, 0x6DFC, 0x6522, 0x6526, 0x653C, 0x7D18, 0x6118, 0x616C, 0x63E7, 0x6102, 0x0000],
            [0x0180, 0x318C, 0x1998, 0x1990, 0x09B0, 0x0180, 0x7FFE, 0x06C0, 0x06C0, 0x04C0, 0x0CC2, 0x08C2, 0x38C6, 0x607E, 0x0000, 0x0000],
            [0x37FE, 0x1804, 0x1804, 0x0004, 0x43E4, 0x7264, 0x1264, 0x0264, 0x0264, 0x13E4, 0x3204, 0x2004, 0x6004, 0x403C, 0x0000, 0x0000]
        ];

        let chars_x_start = [80, 120, 160, 200];
        let cy_start = 100;

        for (i, &cx) in chars_x_start.iter().enumerate() {
            let bitmap = CHARS[i];

            for y in 0..TEXT_H {
                let row = bitmap[y as usize];
                for x in 0..TEXT_W {
                    // 检查第 x 位是否为 1（从左到右，最高位在最左侧）
                    if (row & (1 << (15 - x))) != 0 {
                        // 绘制像素，x 和 y 各乘以 2 放大到 32x32
                        let px = cx + x * 2;
                        let py = cy_start + y * 2;
                        
                        // 强制覆盖颜色，且画满 2x2 的真实方块
                        let set_px = |buf: &mut [u16], px: i32, py: i32| {
                            if px >= 0 && px < fb_w && py >= 0 && py < (buf.len() as i32 / fb_w) {
                                buf[(py * fb_w + px) as usize] = C_NEON_CYAN;
                            }
                        };
                        
                        set_px(buf, px, py);
                        set_px(buf, px + 1, py);
                        set_px(buf, px, py + 1);
                        set_px(buf, px + 1, py + 1);
                    }
                }
            }
        }

        // --- RAW SPI DMA BLIT ---
        // ILI9342C RAMWR command
        dc.set_low().map_err(|e| anyhow::anyhow!("DC low error: {:?}", e))?;
        spi.write(&[0x2C]).map_err(|e| anyhow::anyhow!("SPI cmd error: {:?}", e))?;
        dc.set_high().map_err(|e| anyhow::anyhow!("DC high error: {:?}", e))?;

        // 强转为 byte slice，利用配置好的大 chunk size 进行底层 DMA 发送
        let bytes = unsafe {
            std::slice::from_raw_parts(
                self.fb.buf.as_ptr() as *const u8,
                self.fb.buf.len() * 2,
            )
        };
        
        let chunk_size = 32768;
        for chunk in bytes.chunks(chunk_size) {
            spi.write(chunk).map_err(|e| anyhow::anyhow!("SPI write error: {:?}", e))?;
        }

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
