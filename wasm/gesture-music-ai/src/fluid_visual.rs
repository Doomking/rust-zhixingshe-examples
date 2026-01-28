use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32, // 粒子的生命周期，用于实现淡出效果
}

pub struct Simulator {
    particles: Vec<Particle>,
    max_particles: usize,
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            particles: Vec::with_capacity(500),
            max_particles: 500,
        }
    }

    /// 根据手势坐标更新流体系统
    pub fn update(&mut self, target_x: f32, target_y: f32) -> Vec<f32> {
        // 1. 在当前手势位置生成新粒子
        if self.particles.len() < self.max_particles {
            self.particles.push(Particle {
                x: target_x,
                y: target_y,
                vx: (rand_f32() - 0.5) * 0.01, // 随机初始速度
                vy: (rand_f32() - 0.5) * 0.01,
                life: 1.0,
            });
        }

        // 2. 更新粒子物理状态
        // 我们让粒子向手势中心产生一点吸引力，并模拟流体阻力
        for p in self.particles.iter_mut() {
            let dx = target_x - p.x;
            let dy = target_y - p.y;

            // 简单的流体动力学：加速度指向手势，但带有阻尼
            p.vx += dx * 0.02;
            p.vy += dy * 0.02;
            p.vx *= 0.92; // 阻尼 (Damping)
            p.vy *= 0.92;

            p.x += p.vx;
            p.y += p.vy;
            p.life -= 0.01; // 粒子逐渐老化
        }

        // 3. 移除死亡粒子
        self.particles.retain(|p| p.life > 0.0);

        // 4. 格式化数据：转为简单的平铺数组 [x, y, life, x, y, life...]
        // 这样前端 Three.js 处理起来极快
        let mut render_data = Vec::with_capacity(self.particles.len() * 3);
        for p in &self.particles {
            render_data.push(p.x);
            render_data.push(p.y);
            render_data.push(p.life);
        }
        render_data
    }

    /// 获取流体系统的强度值，基于粒子数量
    /// 返回值范围：0.0 ~ 1.0
    pub fn get_intensity(&self) -> f32 {
        // 基于粒子数量计算强度，最大粒子数为max_particles
        let intensity = self.particles.len() as f32 / self.max_particles as f32;
        // 确保返回值在0.0到1.0之间
        intensity.min(1.0).max(0.0)
    }
}

// 辅助函数：生成简单的随机数
fn rand_f32() -> f32 {
    // WASM 环境下简单的伪随机实现
    let mut seed = 12345.0;
    seed = (seed * 16807.0) % 2147483647.0;
    seed / 2147483647.0
}
