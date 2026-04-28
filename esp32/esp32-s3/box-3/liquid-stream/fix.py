with open('src/physics.rs', 'r') as f:
    content = f.read()

import re

# We will just replace everything from `fn advect_particles` to the `    const SEP_BUCKET: f32 = 6.0;`
pattern = r'    fn advect_particles\(&mut self, dt: f32\) \{.*?    const SEP_BUCKET: f32 = 6\.0;'
replacement = """    fn advect_particles(&mut self, dt: f32) {
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

    const SEP_BUCKET: f32 = 6.0;"""

new_content = re.sub(pattern, replacement, content, flags=re.DOTALL)

with open('src/physics.rs', 'w') as f:
    f.write(new_content)
