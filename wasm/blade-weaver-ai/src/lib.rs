// src/lib.rs
mod filter;
mod geometry;
mod model; // 假设 model.rs 存放之前的 Qwen 加载逻辑

use filter::KalmanFilter;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BladeCore {
    filter: KalmanFilter,
    // model: Option<model::QwenModel>, // 后续集成 LLM
    last_pos: [f32; 2],
}
// --- 供主线程调用的：物理与滤波内核 ---
#[wasm_bindgen]
impl BladeCore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once(); // 崩溃时在浏览器控制台打印错误
        Self {
            filter: KalmanFilter::new(),
            last_pos: [0.0, 0.0],
        }
    }

    // 接口 1：接收原始手势坐标，返回平滑后的坐标
    pub fn update_hand(&mut self, x: f32, y: f32) -> Vec<f32> {
        let smoothed = self.filter.update(x, y);
        self.last_pos = smoothed;
        smoothed.to_vec()
    }

    // 接口 2：暴露我们之前写好的 geometry 切割算法
    pub fn slice_mesh(&self, vertices: &[f32], p_norm: &[f32], p_point: &[f32]) -> JsValue {
        geometry::slice_mesh(vertices, p_norm, p_point)
    }

    // 接口 3：计算切割平面（基于手部移动矢量）
    pub fn calculate_slice_plane(&self, current_pos: &[f32]) -> Vec<f32> {
        // 通过当前位置和上一帧位置的差值，计算切割平面的法线
        let dx = current_pos[0] - self.last_pos[0];
        let dy = current_pos[1] - self.last_pos[1];

        // 法线通常与移动方向垂直
        let mut normal = [-dy, dx, 0.0];
        let len = (normal[0] * normal[0] + normal[1] * normal[1]).sqrt();
        if len > 0.0 {
            normal[0] /= len;
            normal[1] /= len;
        }
        normal.to_vec()
    }
}

// --- 供 Worker 调用的：AI 导演接口 ---
// 这样我们在 JS 端直接 import { GameNarrator } 即可
pub use model::GameNarrator;
