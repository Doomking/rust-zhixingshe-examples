use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{Module, VarBuilder};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*; // 添加这一行
#[derive(Debug, Clone)]
pub struct GestureResult {
    pub id: u32,         // 手势 ID (0-999)
    pub confidence: f32, // 置信度/概率
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)] // 添加 Serialize 和 Deserialize
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub struct Recognizer {
    vision_model: crate::visual_model::BlazeHand,
    // text_model: crate::llm::LLMEngine,
    device: Device,
}

impl Recognizer {
    /// 构造函数：采用引用传递字节数组，减少 WASM 内存占用
    pub fn new(
        vision_weights: &[u8],
        text_weights: &[u8],
        text_tokenizer: &[u8],
        text_config: &[u8],
    ) -> Result<Self> {
        let device = Device::Cpu;

        // 1. 初始化视觉模型：严格校验权重文件
        let vb_vision = VarBuilder::from_buffered_safetensors(
            vision_weights.to_vec(),
            DType::F32, // 改回 F32
            &device,
        )
        .context("视觉权重解析失败")?;

        let vision_model = crate::visual_model::BlazeHand::new(vb_vision)?;

        // 2. 初始化 LLM 引擎：严格校验配置与权重
        // let text_model = crate::llm::LLMEngine::init(text_weights, text_tokenizer, text_config)
        //     .context("LLM 引擎初始化失败")?;

        Ok(Self {
            vision_model,
            // text_model,
            device,
        })
    }

    /// 执行手势识别推理
    pub fn inference(&self, tensor: &Tensor) -> anyhow::Result<Vec<Point>> {
        let output = self.vision_model.forward(tensor)?;
        // 调用我们新写的坐标解码逻辑
        self.decode_landmarks(&output)
    }
    pub fn decode_landmarks(&self, tensor: &Tensor) -> anyhow::Result<Vec<Point>> {
        // tensor 形状 [1, 63] -> 21个点 * (x, y, z)
        let raw_coords = tensor.flatten_all()?.to_vec1::<f32>()?;

        let mut points = Vec::with_capacity(21);
        for i in 0..21 {
            points.push(Point {
                x: raw_coords[i * 3] / 224.0, // 归一化到 0.0 ~ 1.0
                y: raw_coords[i * 3 + 1] / 224.0,
            });
        }
        Ok(points)
    }
    /// 生成赛博诗句
    pub fn generate_text(&mut self, mood: &str) -> String {
        let prompt = format!(
            "<|im_start|>system\n你是一位赛博朋克诗人。<|im_end|>\n\
             <|im_start|>user\n基于意境'{}'写一句7字内的短诗。<|im_end|>\n\
             <|im_start|>assistant\n",
            mood
        );

        // 生产环境下，错误直接记录到浏览器控制台并返回优雅的兜底文案
        // self.text_model
        //     .generate_response(&prompt)
        //     .map_err(|e| {
        //         web_sys::console::error_1(&format!("LLM Inference Error: {:?}", e).into());
        //         e
        //     })
        //     .unwrap_or_else(|_| "赛博流光，指尖微芒".to_string())
        "".to_owned()
    }

    //// 内部逻辑：优化后的 RGBA 预处理
    pub fn preprocess(&self, data: &[u8], w: u32, h: u32) -> anyhow::Result<Tensor> {
        // ImageNet 标准归一化参数
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];

        // 如果日志里 data 全是 0，这里的计算结果就会一直是 -mean/std 得到的负数
        let rgb_data: Vec<f32> = data
            .chunks_exact(4)
            .flat_map(|rgba| {
                [
                    (rgba[0] as f32 / 255.0 - 0.485) / 0.229,
                    (rgba[1] as f32 / 255.0 - 0.456) / 0.224,
                    (rgba[2] as f32 / 255.0 - 0.406) / 0.225,
                ]
            })
            .collect();

        // 构建张量并确保内存连续
        let tensor = Tensor::from_vec(rgb_data, (1, h as usize, w as usize, 3), &self.device)
            .map_err(anyhow::Error::msg)?
            .permute((0, 3, 1, 2))? // NHWC -> NCHW
            .contiguous() // 必须连续，否则后续卷积会 Panic
            .map_err(anyhow::Error::msg)?;

        Ok(tensor)
    }

    /// 结果解码
    fn decode_output(&self, tensor: &Tensor) -> Result<Vec<Point>> {
        // 假设输出 tensor 是 [1, 63]
        let raw_data = tensor.flatten_all()?.to_vec1::<f32>()?;

        let mut points = Vec::with_capacity(21);
        for i in 0..21 {
            // MediaPipe 的坐标通常是归一化的 (0.0 - 1.0)
            points.push(Point {
                x: raw_data[i * 3],
                y: raw_data[i * 3 + 1],
            });
        }
        Ok(points)
    }
}
