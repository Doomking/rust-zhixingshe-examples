use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub position: Vec2,
    pub velocity: Vec2,
}

const SOLID: u8 = 0;
const FLUID: u8 = 1;
const AIR: u8 = 2;

/// 纯正的物理引擎容器，基于 MAC Staggered Grid 架构的 PIC/FLIP 混合算法实现不可压缩流体。
pub struct FluidSim {
    pub particles: Vec<Particle>,

    width: f32,
    height: f32,

    nx: usize,
    ny: usize,
    h: f32,
    inv_h: f32,

    // MAC 交错网格速度场
    u: Vec<f32>,
    v: Vec<f32>,
    u_save: Vec<f32>,
    v_save: Vec<f32>,
    u_weights: Vec<f32>,
    v_weights: Vec<f32>,

    // 以格点中心为基础的数据
    pub density: Vec<u8>, // 每步 P2G 的粒子数统计（不用于「方格块」显示）
    cell_flags: Vec<u8>,

    // PBF 空间哈希分离
    sep_cols: usize,
    sep_rows: usize,
    sep_head: Vec<i32>,
    sep_next: Vec<i32>,
}

impl FluidSim {
    pub fn new(num_particles: usize, width: f32, height: f32) -> Self {
        // 在 ESP32 限制算力下，采用网格分辨率不要太大，网格尺寸越大，计算越快越粗糙
        let h = 10.0; // 每个网格长宽为 10 像素
        let nx = (width / h).ceil() as usize;
        let ny = (height / h).ceil() as usize;
        let num_cells = nx * ny;

        // 贴底「沙堆 / 料堆」：先落在容器下半部的一坨里再被晃散
        let rel_pile_w = 0.86_f32;
        let rel_pile_h = 0.60_f32; // 容纳 1000 个大颗粒
        let margin_x = width * (1.0 - rel_pile_w) * 0.5;
        let pad = h * 0.35;
        let x0 = margin_x + pad;
        let x1 = width - margin_x - pad;
        let y1 = height - pad;
        let y0 = (y1 - height * rel_pile_h).max(pad + h);

        // D：规则网格 + 略小于行距的抖动
        const MIN_S: f32 = 6.0;
        let mut particles: Vec<Particle> = Vec::with_capacity(num_particles);
        let mut row: usize = 0;
        'rows: loop {
            let py_base = y0 + row as f32 * MIN_S;
            if py_base > y1 {
                break;
            }
            let mut col: usize = 0;
            loop {
                let px_base = x0 + col as f32 * MIN_S;
                if px_base > x1 {
                    break;
                }
                if particles.len() >= num_particles {
                    break 'rows;
                }
                let k = particles.len();
                let jitter_x = (((k * 13) % 5) as f32 - 2.0) * 0.18;
                let jitter_y = (((k * 7) % 5) as f32 - 2.0) * 0.18;
                let px = (px_base + jitter_x).clamp(x0, x1);
                let py = (py_base + jitter_y).clamp(y0, y1);
                particles.push(Particle {
                    position: Vec2::new(px, py),
                    velocity: Vec2::ZERO,
                });
                col += 1;
            }
            row += 1;
        }
        if particles.len() < num_particles {
            log::warn!(
                "init: only {} of {} points fit in water rect — use smaller n or rel_water",
                particles.len(),
                num_particles
            );
        }



        const SEP_B: f32 = 6.0;
        let sep_cols = (width / SEP_B).ceil().max(1.0) as usize;
        let sep_rows = (height / SEP_B).ceil().max(1.0) as usize;
        let nsep = sep_cols * sep_rows;
        let sep_head = vec![-1; nsep];
        let sep_next = vec![-1; num_particles];

        let mut sim = Self {
            particles,
            width,
            height,
            nx,
            ny,
            h,
            inv_h: 1.0 / h,
            u: vec![0.0; (nx + 1) * ny],
            v: vec![0.0; nx * (ny + 1)],
            u_save: vec![0.0; (nx + 1) * ny],
            v_save: vec![0.0; nx * (ny + 1)],
            u_weights: vec![0.0; (nx + 1) * ny],
            v_weights: vec![0.0; nx * (ny + 1)],
            density: vec![0; num_cells],
            cell_flags: vec![AIR; num_cells],
            sep_cols,
            sep_rows,
            sep_head,
            sep_next,
        };
        sim.init_walls();
        sim
    }

    fn init_walls(&mut self) {
        // 1. 设置外围边界为固体
        for i in 0..self.nx {
            self.cell_flags[i] = SOLID; // top
            self.cell_flags[(self.ny - 1) * self.nx + i] = SOLID; // bottom
        }
        for j in 0..self.ny {
            self.cell_flags[j * self.nx] = SOLID;
            self.cell_flags[j * self.nx + self.nx - 1] = SOLID;
        }

        // 隐藏的宏观物理墙，用于让流体自然绕开文字区域
        // 文字区域对应的宏观坐标大致为 x:8..23, y:10..13
        for y in 10..14 {
            for x in 8..24 {
                self.cell_flags[y * self.nx + x] = SOLID;
            }
        }
    }

    pub fn grid_dim(&self) -> (usize, usize) {
        (self.nx, self.ny)
    }

    pub fn cell_size(&self) -> f32 {
        self.h
    }

    pub fn grid_density(&self, cx: usize, cy: usize) -> u8 {
        if cx < self.nx && cy < self.ny {
            self.density[cy * self.nx + cx]
        } else {
            0
        }
    }

    pub fn is_solid(&self, cx: usize, cy: usize) -> bool {
        if cx >= self.nx || cy >= self.ny {
            return true;
        }
        self.cell_flags[cy * self.nx + cx] == SOLID
    }

    // 仍为了部分调试用途保留粒子追踪接口
    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }

    /// 调参用：看「流体是否真在动」
    pub fn max_particle_speed(&self) -> f32 {
        self.particles
            .iter()
            .map(|p| p.velocity.length())
            .fold(0.0f32, f32::max)
    }

    pub fn density_at(&self, position: Vec2) -> u8 {
        let cx = (position.x * self.inv_h) as usize;
        let cy = (position.y * self.inv_h) as usize;
        if cx < self.nx && cy < self.ny {
            self.density[cy * self.nx + cx]
        } else {
            0
        }
    }

    fn add_to_u(&mut self, gx: usize, gy: usize, w: f32, value: f32) {
        if gx <= self.nx && gy < self.ny {
            let idx = gy * (self.nx + 1) + gx;
            self.u[idx] += value * w;
            self.u_weights[idx] += w;
        }
    }

    fn add_to_v(&mut self, gx: usize, gy: usize, w: f32, value: f32) {
        if gx < self.nx && gy <= self.ny {
            let idx = gy * self.nx + gx;
            self.v[idx] += value * w;
            self.v_weights[idx] += w;
        }
    }

    fn sample_u(&self, world: Vec2, field: &[f32]) -> f32 {
        let gx = (world.x * self.inv_h).clamp(0.0, self.nx as f32);
        let gy = (world.y * self.inv_h - 0.5).clamp(0.0, (self.ny.saturating_sub(1)) as f32);
        let x0 = gx.floor() as usize;
        let y0 = gy.floor() as usize;
        let x1 = (x0 + 1).min(self.nx);
        let y1 = (y0 + 1).min(self.ny.saturating_sub(1));
        let tx = gx - x0 as f32;
        let ty = gy - y0 as f32;

        let idx00 = y0 * (self.nx + 1) + x0;
        let idx10 = y0 * (self.nx + 1) + x1;
        let idx01 = y1 * (self.nx + 1) + x0;
        let idx11 = y1 * (self.nx + 1) + x1;

        let a = field[idx00] * (1.0 - tx) + field[idx10] * tx;
        let b = field[idx01] * (1.0 - tx) + field[idx11] * tx;
        a * (1.0 - ty) + b * ty
    }

    fn sample_v(&self, world: Vec2, field: &[f32]) -> f32 {
        let gx = (world.x * self.inv_h - 0.5).clamp(0.0, (self.nx.saturating_sub(1)) as f32);
        let gy = (world.y * self.inv_h).clamp(0.0, self.ny as f32);
        let x0 = gx.floor() as usize;
        let y0 = gy.floor() as usize;
        let x1 = (x0 + 1).min(self.nx.saturating_sub(1));
        let y1 = (y0 + 1).min(self.ny);
        let tx = gx - x0 as f32;
        let ty = gy - y0 as f32;

        let idx00 = y0 * self.nx + x0;
        let idx10 = y0 * self.nx + x1;
        let idx01 = y1 * self.nx + x0;
        let idx11 = y1 * self.nx + x1;

        let a = field[idx00] * (1.0 - tx) + field[idx10] * tx;
        let b = field[idx01] * (1.0 - tx) + field[idx11] * tx;
        a * (1.0 - ty) + b * ty
    }

    /// 纯净的 FLIP 步进
    pub fn step(&mut self, dt: f32, gravity: Vec2) {
        for p in &mut self.particles {
            p.velocity += gravity * dt;
        }

        self.transfer_to_grid();
        self.enforce_boundary_conditions();

        self.u_save.copy_from_slice(&self.u);
        self.v_save.copy_from_slice(&self.v);

        // 使用 20 次迭代，保证网格层面流体的宏观不可压缩性
        self.solve_incompressibility(20);

        self.enforce_boundary_conditions();
        self.transfer_to_particles();
        self.advect_particles(dt);

        // PBF 粒子排斥，防止由于重力和 FLIP 限制导致的所有粒子压成一团
        // 采用 3 次强力迭代，绝对锁定水滴的物理体积
        self.push_particles_apart(3);
    }

    fn enforce_boundary_conditions(&mut self) {
        for y in 0..self.ny {
            for x in 0..self.nx {
                if self.cell_flags[y * self.nx + x] == SOLID {
                    self.u[y * (self.nx + 1) + x] = 0.0;
                    self.u[y * (self.nx + 1) + x + 1] = 0.0;
                    self.v[y * self.nx + x] = 0.0;
                    self.v[(y + 1) * self.nx + x] = 0.0;
                }
            }
        }
    }

    fn transfer_to_grid(&mut self) {
        self.u.fill(0.0);
        self.v.fill(0.0);
        self.u_weights.fill(0.0);
        self.v_weights.fill(0.0);
        self.density.fill(0);
        
        // 重置内部非固定单元的标识符
        for idx in 0..self.nx * self.ny {
            if self.cell_flags[idx] != SOLID {
                self.cell_flags[idx] = AIR;
            }
        }

        // 双线性 splat：减少能量锯齿和颗粒感
        for i in 0..self.particles.len() {
            let p = self.particles[i];
            // 标记存在流体的格子并统计密度
            let cx = (p.position.x * self.inv_h) as usize;
            let cy = (p.position.y * self.inv_h) as usize;
            if cx < self.nx && cy < self.ny && self.cell_flags[cy * self.nx + cx] != SOLID {
                self.cell_flags[cy * self.nx + cx] = FLUID;
                self.density[cy * self.nx + cx] = self.density[cy * self.nx + cx].saturating_add(1);
            }

            // u-face splat
            let ux = (p.position.x * self.inv_h).clamp(0.0, self.nx as f32);
            let uy = (p.position.y * self.inv_h - 0.5).clamp(0.0, (self.ny.saturating_sub(1)) as f32);
            let ux0 = ux.floor() as usize;
            let uy0 = uy.floor() as usize;
            let ux1 = (ux0 + 1).min(self.nx);
            let uy1 = (uy0 + 1).min(self.ny.saturating_sub(1));
            let tx = ux - ux0 as f32;
            let ty = uy - uy0 as f32;
            self.add_to_u(ux0, uy0, (1.0 - tx) * (1.0 - ty), p.velocity.x);
            self.add_to_u(ux1, uy0, tx * (1.0 - ty), p.velocity.x);
            self.add_to_u(ux0, uy1, (1.0 - tx) * ty, p.velocity.x);
            self.add_to_u(ux1, uy1, tx * ty, p.velocity.x);

            // v-face splat
            let vx = (p.position.x * self.inv_h - 0.5).clamp(0.0, (self.nx.saturating_sub(1)) as f32);
            let vy = (p.position.y * self.inv_h).clamp(0.0, self.ny as f32);
            let vx0 = vx.floor() as usize;
            let vy0 = vy.floor() as usize;
            let vx1 = (vx0 + 1).min(self.nx.saturating_sub(1));
            let vy1 = (vy0 + 1).min(self.ny);
            let sx = vx - vx0 as f32;
            let sy = vy - vy0 as f32;
            self.add_to_v(vx0, vy0, (1.0 - sx) * (1.0 - sy), p.velocity.y);
            self.add_to_v(vx1, vy0, sx * (1.0 - sy), p.velocity.y);
            self.add_to_v(vx0, vy1, (1.0 - sx) * sy, p.velocity.y);
            self.add_to_v(vx1, vy1, sx * sy, p.velocity.y);
        }

        // 平均化权重
        for i in 0..self.u.len() {
            if self.u_weights[i] > 0.0 {
                self.u[i] /= self.u_weights[i];
            }
        }
        for i in 0..self.v.len() {
            if self.v_weights[i] > 0.0 {
                self.v[i] /= self.v_weights[i];
            }
        }
    }

    fn solve_incompressibility(&mut self, num_iters: usize) {
        let over_relaxation = 1.78;
        
        for _ in 0..num_iters {
            // Gauss-Seidel Method
            for y in 1..self.ny - 1 {
                for x in 1..self.nx - 1 {
                    let idx = y * self.nx + x;
                    if self.cell_flags[idx] != FLUID {
                        continue;
                    }

                    let u_left_idx = y * (self.nx + 1) + x;
                    let u_right_idx = y * (self.nx + 1) + x + 1;
                    let v_down_idx = y * self.nx + x;
                    let v_up_idx = (y + 1) * self.nx + x;

                    let mut sx0 = self.cell_flags[y * self.nx + x - 1] as f32; // 左
                    let mut sx1 = self.cell_flags[y * self.nx + x + 1] as f32; // 右
                    let mut sy0 = self.cell_flags[(y - 1) * self.nx + x] as f32; // 下
                    let mut sy1 = self.cell_flags[(y + 1) * self.nx + x] as f32; // 上

                    // Solid = 0.0, everything else (Fluid/Air) evaluate as fluid domain boundary
                    sx0 = if sx0 == 0.0 { 0.0 } else { 1.0 };
                    sx1 = if sx1 == 0.0 { 0.0 } else { 1.0 };
                    sy0 = if sy0 == 0.0 { 0.0 } else { 1.0 };
                    sy1 = if sy1 == 0.0 { 0.0 } else { 1.0 };

                    let s = sx0 + sx1 + sy0 + sy1;
                    if s == 0.0 {
                        continue;
                    }

                    let div = (self.u[u_right_idx] - self.u[u_left_idx] + self.v[v_up_idx] - self.v[v_down_idx]) * self.inv_h;
                    let diff_p = -div / s * over_relaxation;

                    self.u[u_left_idx] -= sx0 * diff_p;
                    self.u[u_right_idx] += sx1 * diff_p;
                    self.v[v_down_idx] -= sy0 * diff_p;
                    self.v[v_up_idx] += sy1 * diff_p;
                }
            }
        }
    }

    fn transfer_to_particles(&mut self) {
        // 适当保留 FLIP 惯性，让水流不会过于僵硬，更灵动
        let flip_ratio = 0.95_f32;

        for i in 0..self.particles.len() {
            let p = self.particles[i];
            let u_pic = self.sample_u(p.position, &self.u);
            let v_pic = self.sample_v(p.position, &self.v);
            let u_prev = self.sample_u(p.position, &self.u_save);
            let v_prev = self.sample_v(p.position, &self.v_save);
            let u_flip_diff = u_pic - u_prev;
            let v_flip_diff = v_pic - v_prev;

            // 动能回写
            let u_flip = p.velocity.x + u_flip_diff;
            let v_flip = p.velocity.y + v_flip_diff;
            let mut new_vx = flip_ratio * u_flip + (1.0 - flip_ratio) * u_pic;
            let mut new_vy = flip_ratio * v_flip + (1.0 - flip_ratio) * v_pic;

            // 限制最大速度，防止边界箝位与挤压带来的数值爆炸
            let max_v = 1000.0;
            let v_sq = new_vx * new_vx + new_vy * new_vy;
            if v_sq > max_v * max_v {
                let scale = max_v / v_sq.sqrt();
                new_vx *= scale;
                new_vy *= scale;
            }

            self.particles[i].velocity.x = new_vx;
            self.particles[i].velocity.y = new_vy;
        }
    }

    fn advect_particles(&mut self, dt: f32) {
        let steps = 3;
        let dt_sub = dt / steps as f32;

        for p in &mut self.particles {
            for _ in 0..steps {
                p.position += p.velocity * dt_sub;
            }
            
            // 极限约束，防止数值误差导致粒子飞出网格导致内存越界
            // 这里保留了 1.01 * h 的安全边界，确保粒子中心始终处于流体网格内部的非 SOLID 区域
            let margin_x = self.h * 1.01;
            let margin_y = self.h * 1.01;

            p.position.x = p.position.x.clamp(margin_x, self.width - margin_x);
            p.position.y = p.position.y.clamp(margin_y, self.height - margin_y);
        }
    }

    const SEP_BUCKET: f32 = 6.0;

    fn push_particles_apart(&mut self, num_iters: usize) {
        let sc = self.sep_cols;
        let sr = self.sep_rows;
        let target = 6.0; // 强制保持的粒子直径
        let overlap_threshold = 6.0;

        for _ in 0..num_iters {
            self.sep_head.fill(-1);
            if self.sep_next.len() != self.particles.len() {
                self.sep_next.resize(self.particles.len(), -1);
            }
            self.sep_next.fill(-1);

            // 构建桶内单链表
            for i in 0..self.particles.len() {
                let p = self.particles[i].position;
                let mut cx = (p.x / Self::SEP_BUCKET).floor() as i32;
                let mut cy = (p.y / Self::SEP_BUCKET).floor() as i32;
                cx = cx.clamp(0, (sc as i32) - 1);
                cy = cy.clamp(0, (sr as i32) - 1);
                let bidx = (cy as usize) * sc + (cx as usize);

                self.sep_next[i] = self.sep_head[bidx];
                self.sep_head[bidx] = i as i32;
            }

            // 一轮邻域松弛：只对 j > i 处理一次
            for i in 0..self.particles.len() {
                let p = self.particles[i].position;
                let mut cx = (p.x / Self::SEP_BUCKET).floor() as i32;
                let mut cy = (p.y / Self::SEP_BUCKET).floor() as i32;
                cx = cx.clamp(0, (sc as i32) - 1);
                cy = cy.clamp(0, (sr as i32) - 1);
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let nx = cx + ox;
                        let ny = cy + oy;
                        if nx < 0 || ny < 0 || nx >= sc as i32 || ny >= sr as i32 {
                            continue;
                        }
                        let nidx = (ny as usize) * sc + (nx as usize);
                        let mut count = 0;
                        let mut j = self.sep_head[nidx];
                        while j >= 0 && count < 10 {
                            let ju = j as usize;
                            if ju > i {
                                self.separate_pair(i, ju, overlap_threshold, target);
                            }
                            count += 1;
                            j = self.sep_next[ju];
                        }
                    }
                }
            }
        }
        
        let w = self.width;
        let h = self.height;
        for p in &mut self.particles {
            p.position.x = p.position.x.clamp(self.h * 1.01, w - self.h * 1.01);
            p.position.y = p.position.y.clamp(self.h * 1.01, h - self.h * 1.01);

            // 强制将粒子推离文字的粗略边界框，防止卡死
            if p.position.x > 76.0 && p.position.x < 236.0 && p.position.y > 96.0 && p.position.y < 136.0 {
                let dx1 = p.position.x - 76.0;
                let dx2 = 236.0 - p.position.x;
                let dy1 = p.position.y - 96.0;
                let dy2 = 136.0 - p.position.y;
                
                let min_d = dx1.min(dx2).min(dy1).min(dy2);
                if min_d == dx1 { p.position.x = 76.0; p.velocity.x *= -0.5; }
                else if min_d == dx2 { p.position.x = 236.0; p.velocity.x *= -0.5; }
                else if min_d == dy1 { p.position.y = 96.0; p.velocity.y *= -0.5; }
                else { p.position.y = 136.0; p.velocity.y *= -0.5; }
            }
        }
    }

    fn separate_pair(&mut self, i: usize, j: usize, overlap_threshold: f32, target: f32) {
        let (a, b) = if i < j { (i, j) } else { (j, i) };
        let (left, right) = self.particles.split_at_mut(b);
        let p1 = &mut left[a].position;
        let p2 = &mut right[0].position;
        let d = *p2 - *p1;
        let dist = d.length();
        if dist >= overlap_threshold {
            return;
        }
        if dist < 1e-4 {
            let s = if (a + b) & 1 == 0 { 1.0 } else { -1.0 };
            *p1 = *p1 - Vec2::new(s * 0.1, 0.0);
            *p2 = *p2 + Vec2::new(s * 0.1, 0.0);
            return;
        }
        let n = d / dist;
        // 强刚性挤压维持绝对积体，不让重力将其压扁
        let push = 0.5 * (target - dist) * 0.8;
        *p1 -= n * push;
        *p2 += n * push;
    }
}

// ==========================================
// Phase 6: 质量保证与发布 - 内部无依赖核心流体物理沙盒测试
// ==========================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fluid_initialization() {
        let sim = FluidSim::new(200, 320.0, 240.0);
        assert_eq!(sim.particles().len(), 200, "Should correctly initialize requested particle count");
        
        let expected_nx = 32; // 320 / 10
        let expected_ny = 24; // 240 / 10
        assert_eq!(sim.nx, expected_nx, "Grid X dimension incorrectly calculated");
        assert_eq!(sim.ny, expected_ny, "Grid Y dimension incorrectly calculated");
    }

    #[test]
    fn test_static_obstacle_masking() {
        let sim = FluidSim::new(100, 320.0, 240.0);
        // Test Outer edge boundaries
        assert!(sim.is_solid(0, 0), "Top-Left corner should be Solid");
        assert!(sim.is_solid(sim.nx - 1, sim.ny - 1), "Bottom-Right corner should be Solid");
        
        // Test Inner fluid volume (AIR/FLUID)
        assert!(!sim.is_solid(2, 2), "Inner area should be permissive (not solid)");
        assert!(!sim.is_solid(sim.nx / 2, sim.ny / 2), "Center should not be solid");
    }

    #[test]
    fn test_physics_step_energy_injection() {
        let mut sim = FluidSim::new(10, 320.0, 240.0);
        let p_initial = sim.particles()[0];
        
        // Apply downwards gravity field
        sim.step(0.1, Vec2::new(0.0, 9.8));
        
        let p_first_step = sim.particles()[0];
        assert!(p_first_step.velocity.y > p_initial.velocity.y, "Gravity should accelerate particles downwards");
        assert!(p_first_step.position.y > p_initial.position.y, "Particles should shift vertically due to gravity");
    }

    #[test]
    fn test_density_gathering() {
        let mut sim = FluidSim::new(4, 320.0, 240.0);
        // Move step forward to run P2G and density splat
        sim.step(0.1, Vec2::new(0.0, 10.0));
        
        let mut total_active_cells = 0;
        for &d in &sim.density {
            if d > 0 {
                total_active_cells += 1;
            }
        }
        assert!(total_active_cells > 0, "Density map should be populated during P2G step");
    }
}
