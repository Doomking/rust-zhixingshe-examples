with open('src/physics.rs', 'r') as f:
    content = f.read()

import re

# 1. Update init_walls
old_init = """    fn init_walls(&mut self) {
        // 1. 设置外围边界为固体
        for i in 0..self.nx {
            self.cell_flags[i] = SOLID; // top
            self.cell_flags[(self.ny - 1) * self.nx + i] = SOLID; // bottom
        }
        for j in 0..self.ny {
        }
    }"""
new_init = """    fn init_walls(&mut self) {
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
        // 文字区域对应的宏观坐标大致为 x:8..24, y:10..14
        for y in 10..14 {
            for x in 8..24 {
                self.cell_flags[y * self.nx + x] = SOLID;
            }
        }
    }"""

content = content.replace(old_init, new_init)

# 2. Update advect_particles
content = re.sub(r'    fn advect_particles\(&mut self, dt: f32\) \{.*?    \}', 
"""    fn advect_particles(&mut self, dt: f32) {
        let steps = 3;
        let dt_sub = dt / steps as f32;

        for p in &mut self.particles {
            for _ in 0..steps {
                p.position += p.velocity * dt_sub;
            }
        }
    }""", content, flags=re.DOTALL)

# 3. Update bucket limit in push_particles_apart
old_bucket = """                // 限制每个桶的粒子数量，彻底杜绝粒子重叠引发的 O(N^2) 性能暴跌
                let mut len = 0;
                let mut curr = self.sep_head[bidx];
                while curr >= 0 {
                    len += 1;
                    curr = self.sep_next[curr as usize];
                }
                if len < 30 {
                    self.sep_next[i] = self.sep_head[bidx];
                    self.sep_head[bidx] = i as i32;
                }"""
new_bucket = """                self.sep_next[i] = self.sep_head[bidx];
                self.sep_head[bidx] = i as i32;"""
content = content.replace(old_bucket, new_bucket)

# 4. Update check limit in push_particles_apart
old_check = """                        let mut j = self.sep_head[nidx];
                        while j >= 0 {
                            let ju = j as usize;
                            if ju > i {
                                self.separate_pair(i, ju, overlap_threshold, target);
                            }
                            j = self.sep_next[ju];
                        }"""
new_check = """                        let mut j = self.sep_head[nidx];
                        let mut count = 0;
                        while j >= 0 && count < 10 { // 限制单格检测数量，既保证体积又避免性能崩溃
                            let ju = j as usize;
                            if ju > i {
                                self.separate_pair(i, ju, overlap_threshold, target);
                            }
                            j = self.sep_next[ju];
                            count += 1;
                        }"""
content = content.replace(old_check, new_check)

# 5. Add boundary repeller
old_bound = """        for p in &mut self.particles {
            p.position.x = p.position.x.clamp(self.h * 1.01, w - self.h * 1.01);
            p.position.y = p.position.y.clamp(self.h * 1.01, h - self.h * 1.01);
        }"""
new_bound = """        for p in &mut self.particles {
            p.position.x = p.position.x.clamp(self.h * 1.01, w - self.h * 1.01);
            p.position.y = p.position.y.clamp(self.h * 1.01, h - self.h * 1.01);

            // 粗略防止粒子进入文字区域导致视觉穿模
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
        }"""
content = content.replace(old_bound, new_bound)

with open('src/physics.rs', 'w') as f:
    f.write(content)
print("Done")
