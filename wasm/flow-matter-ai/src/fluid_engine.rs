use crate::model::MaterialProperties;

pub struct FluidEngine {
    pub width: u32,
    pub height: u32,
    // 粒子系统：存储数千个流体粒子
    pub particles: Vec<Particle>,
    // 当前激活的物质属性
    pub current_material: MaterialProperties,
    // 原图尺寸，用于采样
    pub img_dims: (u32, u32),
    pub scaled_dims: (u32, u32),
}

pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
    pub decay: f32,
    pub mass: f32,
    pub color: (u8, u8, u8),
}

impl FluidEngine {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            particles: Vec::with_capacity(2000),
            current_material: MaterialProperties::default(),
            img_dims: (1, 1),
            scaled_dims: (1, 1),
        }
    }

    /// 核心功能 1：物质提取 (Material Extraction)
    /// 当 SAM 返回 mask 后，在选中区域注入粒子
    pub fn inject_material(
        &mut self,
        mask: &[u8],
        img_w: u32,
        img_h: u32,
        scaled_w: u32,
        scaled_h: u32,
        offset_x: f32,
        offset_y: f32,
        unit_scale_x: f32,
        unit_scale_y: f32,
        material: MaterialProperties,
    ) {
        self.img_dims = (img_w, img_h);
        self.scaled_dims = (scaled_w, scaled_h);
        self.current_material = material.clone();

        let mut rng = rand::thread_rng();
        use rand::Rng;

        // 核心修复 1：统计活跃像素，实现随机概率注入，避免“只出现在顶部”
        let active_pixels = mask.iter().filter(|&&v| v > 0).count();
        if active_pixels == 0 {
            return;
        }

        let target_new_particles = 1500; // 每次点击注入的期望数量
        let spawn_prob = (target_new_particles as f32 / active_pixels as f32).min(1.0);
        let max_total_particles = 6000; // 提升总上限

        for (idx, &is_selected) in mask.iter().enumerate() {
            if is_selected > 0 && self.particles.len() < max_total_particles {
                // 随机抽样实现全域均匀分布
                if !rng.gen_bool(spawn_prob as f64) {
                    continue;
                }

                let mask_x = (idx as u32 % 256) as f32;
                let mask_y = (idx as u32 / 256) as f32;

                // 核心修复 2：使用 (val + 0.5) 将粒子置于 4x4 网格中心，减少系统性偏移
                let x = offset_x + (mask_x + 0.5) * 4.0 * unit_scale_x;
                let y = offset_y + (mask_y + 0.5) * 4.0 * unit_scale_y;

                // 极致颜色多样性：基于色相的 HSL 扰动
                let h_jitter = rng.gen_range(-25.0..25.0);
                let final_h = (material.hue + h_jitter + 360.0) % 360.0;
                let final_s = rng.gen_range(0.6..1.0);
                let final_l = rng.gen_range(0.3..0.8);

                let (r, g, b) = hsl_to_rgb(final_h, final_s, final_l);

                self.particles.push(Particle {
                    x,
                    y,
                    vx: rng.gen_range(-3.5..3.5), // 增强扩散力度
                    vy: rng.gen_range(-3.5..3.5),
                    life: 1.0,
                    decay: rng.gen_range(0.002..0.008), // 生命周期适度延长
                    mass: rng.gen_range(0.7..1.3),
                    color: (r, g, b),
                });
            }
        }
    }

    /// 核心功能 3：空间塌陷 (The Collapse)
    /// 让物体瞬间流体化并向下坠落
    pub fn apply_collapse(&mut self, audio_force: f32) {
        let gravity = 0.5;

        for p in self.particles.iter_mut() {
            // 物理坠落
            p.vy += gravity;
            // 频谱震荡：受音频低音驱动产生水平撕裂感
            p.vx += (audio_force * 10.0) * (rand::random::<f32>() - 0.5);

            p.x += p.vx;
            p.y += p.vy;
        }
    }

    pub fn step(&mut self, avg_audio: f32, mouse_x: f32, mouse_y: f32) {
        // 物理常数
        let flow_intensity = 0.2 * (1.0 + avg_audio * 5.0);
        let gravity = 0.05 * (1.0 + avg_audio * 2.0);
        let friction = 0.98;
        let mouse_radius = 200.0;
        let mouse_strength = 1.5;

        for p in self.particles.iter_mut() {
            // 1. 流场 (Flow Field) 模拟
            // 使用正弦波模拟简易漩涡，随鼠标位置微调
            let angle = (p.x * 0.01).sin() * 2.0 + (p.y * 0.01).cos() * 2.0;
            p.vx += angle.cos() * flow_intensity / p.mass;
            p.vy += angle.sin() * flow_intensity / p.mass;

            // 2. 重力
            p.vy += gravity;

            // 3. 鼠标交互 (超级吸引)
            let dx = mouse_x - p.x;
            let dy = mouse_y - p.y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq < mouse_radius * mouse_radius && dist_sq > 1.0 {
                let dist = dist_sq.sqrt();
                //  attraction 增强，且带有旋转分量增加动感
                let force = (1.0 - dist / mouse_radius) * mouse_strength * 3.5;
                p.vx += (dx / dist) * force;
                p.vy += (dy / dist) * force;

                // 涡旋效果
                let v_force = force * 0.2;
                p.vx += (dy / dist) * v_force;
                p.vy -= (dx / dist) * v_force;
            }

            // 4. 应用速度
            p.x += p.vx;
            p.y += p.vy;
            p.vx *= friction;
            p.vy *= friction;

            // 5. 衰减
            p.life -= p.decay;

            // 6. 边界约束 (弹性)
            if p.x < 0.0 {
                p.x = 0.0;
                p.vx *= -0.5;
            }
            if p.x > self.width as f32 {
                p.x = self.width as f32;
                p.vx *= -0.5;
            }
            if p.y < 0.0 {
                p.y = 0.0;
                p.vy *= -0.5;
            }
            if p.y > self.height as f32 {
                p.y = self.height as f32;
                p.vy *= -0.5;
            }
        }
        self.particles.retain(|p| p.life > 0.0);
    }

    pub fn get_render_data(&self) -> Vec<f32> {
        // 返回扁平化的粒子坐标和颜色数组 [x, y, r, g, b, life, ...]
        let mut data = Vec::with_capacity(self.particles.len() * 6);
        for p in &self.particles {
            data.push(p.x);
            data.push(p.y);
            data.push(p.color.0 as f32 / 255.0);
            data.push(p.color.1 as f32 / 255.0);
            data.push(p.color.2 as f32 / 255.0);
            data.push(p.life);
        }
        data
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).clamp(0.0, 255.0) as u8,
    )
}
