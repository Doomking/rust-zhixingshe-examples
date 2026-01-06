use wasm_bindgen::prelude::*;

/// 计算两个 512 维向量的余弦相似度
/// 模拟 CLIP 等模型在 AI 检索时的核心计算任务
#[wasm_bindgen]
pub fn cosine_similarity_rust(vec_a: &[f32], vec_b: &[f32]) -> f32 {
    if vec_a.len() != vec_b.len() || vec_a.is_empty() {
        return 0.0;
    }

    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for i in 0..vec_a.len() {
        dot_product += vec_a[i] * vec_b[i];
        norm_a += vec_a[i] * vec_a[i];
        norm_b += vec_b[i] * vec_b[i];
    }

    let denominator = norm_a.sqrt() * norm_b.sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        dot_product / denominator
    }
}

#[wasm_bindgen]
pub fn cosine_similarity_rust_benchmark(vec_a: &[f32], vec_b: &[f32], iterations: i32) -> f32 {
    let mut last_result = 0.0;
    for _ in 0..iterations {
        last_result = cosine_similarity_rust(vec_a, vec_b);
    }
    last_result
}