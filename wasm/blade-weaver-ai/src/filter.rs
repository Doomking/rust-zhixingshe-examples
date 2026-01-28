// src/filter.rs
use nalgebra::{Matrix2, Vector2};

pub struct KalmanFilter {
    // 状态向量 [x, y, vx, vy]：位置和速度
    state: [f32; 4],
    // 协方差矩阵：代表我们对当前状态的信任度
    p: [[f32; 4]; 4],
    // 预测噪声
    q: f32,
    // 测量噪声
    r: f32,
}

impl KalmanFilter {
    pub fn new() -> Self {
        let mut p = [[0.0; 4]; 4];
        for i in 0..4 {
            p[i][i] = 1.0;
        }

        Self {
            state: [0.0; 4],
            p,
            q: 0.1, // 越小越平滑，越大反应越快
            r: 0.5, // 测量噪声
        }
    }

    pub fn update(&mut self, z_x: f32, z_y: f32) -> [f32; 2] {
        // 1. 预测 (Predict)
        // 状态转移矩阵：x_new = x + vx, y_new = y + vy
        self.state[0] += self.state[2];
        self.state[1] += self.state[3];

        // 2. 更新 (Update)
        let k_x = self.p[0][0] / (self.p[0][0] + self.r);
        let k_y = self.p[1][1] / (self.p[1][1] + self.r);

        self.state[0] += k_x * (z_x - self.state[0]);
        self.state[1] += k_y * (z_y - self.state[1]);

        // 更新速度估计
        self.state[2] = k_x * (z_x - self.state[0]);
        self.state[3] = k_y * (z_y - self.state[1]);

        [self.state[0], self.state[1]]
    }
}
