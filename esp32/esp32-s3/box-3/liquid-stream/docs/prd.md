这是一份为你定制的完整产品技术文档（PRD + Tech Spec），旨在统一产品愿景与技术执行。

---

# 🚀 产品技术文档：LiquidStream (光流) 
**项目代号**：LiquidStream  
**品牌所属**：无限光河 (Infinite Light River)  
**目标硬件**：ESP32-S3 BOX 3  
**核心技术**：Rust (STD), FLIP 流体算法, BMI270 惯性导航  

---

## 一、 产品概述 (Product Vision)

### 1.1 背景与目标
LiquidStream 旨在利用 ESP32-S3 的高性能双核性能，在 320x240 的屏幕上实现一套高度真实的物理流体交互系统。不同于传统的像素沙盒，本项目追求的是**“不可压缩液体”**的质感，模拟真实水的波纹、惯性和堆积感。

### 1.2 核心体验
* **重力感应**：液体方向随设备物理倾斜而改变，仿佛屏幕里真的装了一袋水。
* **交互反馈**：流体与边缘、甚至预设的 UI 元素（如 Logo）产生动态碰撞。
* **视觉美学**：通过色彩映射和尾迹算法，展现出一种具有未来感的“电子流体”效果。

---

## 二、 功能规格 (Functional Requirements)

| 功能模块 | 描述 | 关键指标 |
| :--- | :--- | :--- |
| **物理引擎** | 基于 FLIP 算法的流体动力学计算 | 稳定 30+ FPS，支持 1000+ 粒子 |
| **感知交互** | 获取 BMI270 加速度计数据并映射至全局力场 | 响应延迟 < 20ms |
| **渲染系统** | 将物理状态绘制到 LCD 屏幕 | 支持双缓冲，无撕裂感 |
| **静态碰撞** | 流体能够绕过特定区域（如“无限光河”Logo） | 支持自定义碰撞掩码 |

---

## 三、 技术架构 (Technical Specification)

### 3.1 核心算法：FLIP (Fluid-Implicit Particle)
系统采用粒子与网格混合的解法：
1.  **粒子层**：存储位置和速度，表现水的流动。
2.  **网格层**：计算压力差，确保流体不会像沙子一样坍塌，而是保持体积感。



### 3.2 软件栈
* **运行环境**：`esp-idf-hal` (Standard Library support)。
* **计算库**：`glam`（处理向量运算，充分利用 S3 的 FPU）。
* **外设管理**：
    * **SPI/DMA**：用于 20MHz+ 的高速屏幕刷新。
    * **I2C**：用于 BMI270 传感器数据读取。

---

## 四、 代码实现：LiquidStream 核心逻辑

为了实现“一键上传并运行”，我们需要构建一个整洁的工程结构。以下是基于 Rust (STD) 的核心仿真框架。

### 4.1 项目结构建议
```text
LiquidStream/
├── Cargo.toml          # 依赖管理
├── sdkconfig.defaults  # 开启 PSRAM 和 FPU 优化
└── src/
    ├── main.rs         # 硬件初始化与主循环
    ├── fluid.rs        # FLIP 物理引擎核心
    └── imu.rs          # 传感器数据处理
```

### 4.2 核心物理引擎原型 (fluid.rs)
```rust
use glam::Vec2;

pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
}

pub struct FluidSim {
    pub particles: Vec<Particle>,
    grid_size: (usize, usize),
    gravity: Vec2,
    dt: f32,
}

impl FluidSim {
    pub fn new(width: f32, height: f32, num_particles: usize) -> Self {
        let mut particles = Vec::with_capacity(num_particles);
        for i in 0..num_particles {
            particles.push(Particle {
                pos: Vec2::new(i as f32 % width, i as f32 / width),
                vel: Vec2::ZERO,
            });
        }
        Self {
            particles,
            grid_size: (40, 30), // 降采样网格
            gravity: Vec2::new(0.0, 9.8),
            dt: 0.033, // 目标 30 FPS
        }
    }

    // 更新重力（由 IMU 调用）
    pub fn update_gravity(&mut self, new_gravity: Vec2) {
        self.gravity = new_gravity;
    }

    // 核心仿真步骤
    pub fn step(&mut self) {
        for p in self.particles.iter_mut() {
            // 1. 施加重力
            p.vel += self.gravity * self.dt;
            // 2. 位置更新 (简单积分)
            p.pos += p.vel * self.dt;

            // 3. 边界碰撞逻辑
            if p.pos.x < 0.0 { p.pos.x = 0.0; p.vel.x *= -0.5; }
            if p.pos.x > 320.0 { p.pos.x = 320.0; p.vel.x *= -0.5; }
            if p.pos.y < 0.0 { p.pos.y = 0.0; p.vel.y *= -0.5; }
            if p.pos.y > 240.0 { p.pos.y = 240.0; p.vel.y *= -0.5; }
        }
        // TODO: 在此处加入 Grid-based 压力解算以实现真正的流体感
    }
}
```

---

## 五、 部署与自动化 (DevOps)

### 5.1 一键上传配置
为了实现你的“一键部署”要求，请在项目根目录配置 `.cargo/config.toml`：

```toml
[target.xtensa-esp32s3-espidf]
linker = "ldproxy"
runner = "espflash flash --monitor" # 运行 cargo run 自动烧录并监控
```

### 5.2 编译优化建议
在 `Cargo.toml` 中开启最高级别的优化，这对物理引擎至关重要：
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
```

---

## 六、 质量保证 (QA)

* **性能监控**：在主循环中测量 `step()` 函数耗时。如果耗时 > 33ms，则自动减少粒子数量。
* **稳定性测试**：设备倾斜 90 度并保持，观察流体是否会在角落产生数值溢出或异常抖动。
* **单元测试**：
    * `test_gravity_vector`：验证加速度计原始数据正确转换为 $f32$。
    * `test_boundary_safety`：验证极端速度下粒子不会穿透屏幕。

---

## 结论与行动方案

这份文档为你提供了从算法原理到工程部署的全套蓝图。你可以直接基于此结构编写代码，利用 Rust 的内存安全特性，规避掉 C 语言在嵌入式开发中常见的指针错误。

**接下来你可以直接执行：**
1. 使用 `esp-idf-template` 初始化项目。
2. 将上述 `fluid.rs` 逻辑填入。
3. 连接 BOX 3，执行 `cargo run --release`。

如果你准备好了，我可以针对 **Jacobi 压力迭代** 部分提供更深入的代码实现，那是让流体“动起来”最灵魂的一步。需要我现在写出这部分算法吗？