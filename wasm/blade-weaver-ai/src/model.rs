// src/model.rs
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen2::{Config as QwenConfig, ModelForCausalLM as QwenModel};
use tokenizers::Tokenizer;
use wasm_bindgen::prelude::*;

// 复用你提供的核心 LLM 引擎逻辑
pub struct LLMEngine {
    model: QwenModel,
    tokenizer: Tokenizer,
    device: Device,
    pos: usize,
    token_ids: Vec<u32>,
    last_decoded_len: usize,
    is_finished: bool,
}

impl LLMEngine {
    pub async fn init(
        weights_data: Vec<u8>,
        tokenizer_data: Vec<u8>,
        config_data: Vec<u8>,
    ) -> Result<LLMEngine, JsValue> {
        console_error_panic_hook::set_once();

        // 由于后端兼容性，暂回退至 CPU 推理
        let device = Device::Cpu;

        let tokenizer = Tokenizer::from_bytes(&tokenizer_data)
            .map_err(|e| JsValue::from_str(&format!("Tokenizer Error: {:?}", e)))?;

        let config: QwenConfig = serde_json::from_slice(&config_data)
            .map_err(|e| JsValue::from_str(&format!("Config JSON Error: {:?}", e)))?;

        let vb = VarBuilder::from_buffered_safetensors(weights_data, DType::F32, &device)
            .map_err(|e| JsValue::from_str(&format!("Weights Error: {:?}", e)))?;

        let model = QwenModel::new(&config, vb)
            .map_err(|e| JsError::new(&format!("Model Init Error: {:?}", e)))?;

        Ok(Self {
            model,
            tokenizer,
            device,
            pos: 0,
            token_ids: Vec::new(),
            last_decoded_len: 0,
            is_finished: false,
        })
    }

    pub fn apply_prompt(&mut self, prompt: &str) -> Result<(), JsError> {
        self.reset();
        let tokens = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| JsError::new(&format!("Tokenize error: {:?}", e)))?;
        self.token_ids = tokens.get_ids().to_vec();

        // 预先解码 Prompt 的长度，作为流式输出的起点
        let decoded = self
            .tokenizer
            .decode(&self.token_ids, false)
            .map_err(|e| JsError::new(&format!("Decode error: {:?}", e)))?;
        self.last_decoded_len = decoded.len();
        self.is_finished = false;

        Ok(())
    }

    pub fn step(&mut self) -> Result<String, JsError> {
        let eos_id = 151643;
        let im_start_id = 151644;
        let im_end_id = 151645;

        if self.token_ids.is_empty() {
            return Ok("".to_string());
        }

        // 增量推理逻辑
        let context_size = if self.pos == 0 {
            self.token_ids.len()
        } else {
            1
        };
        let start_idx = self.token_ids.len() - context_size;

        let input_tensor = Tensor::new(&self.token_ids[start_idx..], &self.device)
            .map_err(|e| JsError::new(&e.to_string()))?
            .unsqueeze(0)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // ModelForCausalLM 的 forward 处理了最后一位的 logits，返回形状为 [1, 1, Vocab]
        let logits = self
            .model
            .forward(&input_tensor, self.pos)
            .map_err(|e| JsError::new(&e.to_string()))?;

        self.pos += context_size;

        // 使用 temperature (0.7) 采样并扁平化为 1D [Vocab]
        let logits = (logits / 0.7)
            .map_err(|e| JsError::new(&e.to_string()))?
            .flatten_all()
            .map_err(|e| JsError::new(&e.to_string()))?;

        // 应用重复惩罚 (Repetition Penalty)
        // 惩罚系数建议在 1.1 左右，防止模型陷入死循环
        let penalty = 1.1f32;
        let mut logits_vec = logits
            .to_vec1::<f32>()
            .map_err(|e| JsError::new(&e.to_string()))?;

        for &id in &self.token_ids {
            let idx = id as usize;
            if idx < logits_vec.len() {
                if logits_vec[idx] > 0.0 {
                    logits_vec[idx] /= penalty;
                } else {
                    logits_vec[idx] *= penalty;
                }
            }
        }

        let logits = Tensor::from_vec(logits_vec, logits.dims(), &self.device)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let next_token_id = logits
            .argmax(0)
            .map_err(|e| JsError::new(&e.to_string()))?
            .to_scalar::<u32>()
            .map_err(|e| JsError::new(&e.to_string()))?;

        if next_token_id == eos_id || next_token_id == im_start_id || next_token_id == im_end_id {
            self.is_finished = true;
            return Ok("".to_string()); // 返回空字符串表示结束
        }

        self.token_ids.push(next_token_id);

        // 解码整个序列（包含 prompt），然后计算增量部分
        let all_text = self
            .tokenizer
            .decode(&self.token_ids, false)
            .map_err(|e| JsError::new(&format!("Decode error: {:?}", e)))?;

        let new_text = &all_text[self.last_decoded_len..];

        // 如果增量部分以 UTF-8 替代字符 (U+FFFD) 结尾，说明字符被截断了，等待下一个 token
        if new_text.ends_with('\u{FFFD}') {
            return Ok("".to_string());
        }

        // 更新解码位置偏移量
        self.last_decoded_len = all_text.len();

        Ok(new_text.to_string())
    }

    pub fn is_finished(&self) -> bool {
        self.is_finished
    }

    pub fn generate_response(&mut self, prompt: &str) -> Result<String, JsError> {
        self.apply_prompt(prompt)?;
        let mut text = String::new();
        for _ in 0..64 {
            let piece = self.step()?;
            if piece.is_empty() {
                break;
            }
            text.push_str(&piece);
        }
        Ok(text.trim().to_string())
    }

    pub fn reset(&mut self) {
        // 清除模型内部的 KV cache
        self.model.clear_kv_cache();
        self.pos = 0;
        self.token_ids.clear();
        self.is_finished = false;
    }
}

#[wasm_bindgen]
pub struct GameNarrator {
    engine: LLMEngine,
}

#[wasm_bindgen]
impl GameNarrator {
    // 供 JS Worker 调用初始化
    pub async fn new(
        weights: Vec<u8>,
        tokenizer: Vec<u8>,
        config: Vec<u8>,
    ) -> Result<GameNarrator, JsValue> {
        let engine = LLMEngine::init(weights, tokenizer, config).await?;
        Ok(Self { engine })
    }

    // 【核心增强】：根据游戏事件生成指令和解说
    pub fn process_game_event(&mut self, event_json: &str) -> String {
        let event: serde_json::Value = serde_json::from_str(event_json).unwrap_or_default();

        // 1. 构建适合 Qwen 的 Prompt 模板
        let action = event["action"].as_str().unwrap_or("none");
        let prompt = match action {
            "game_start" =>
                "<|im_start|>system\n你是一个热血的格斗游戏解说员。游戏开始了，请说一句简短的开场白。<|im_end|>\n<|im_start|>assistant\n".to_string(),
            "slice" => {
                let combo = event["combo"].as_i64().unwrap_or(0);
                format!(
                    "<|im_start|>system\n你是一个热血解说。玩家刚刚切开了水果，当前连击数是 {}。请用 15 字以内的一句话夸张地赞美他！不要重复。<|im_end|>\n<|im_start|>assistant\n",
                    combo
                )
            },
            "miss" =>
                "<|im_start|>system\n玩家漏掉了水果。请用幽默、毒舌的一句话嘲讽他，激励他继续努力。<|im_end|>\n<|im_start|>assistant\n".to_string(),
            _ => "<|im_start|>system\n说一句战斗格言。<|im_end|>\n<|im_start|>assistant\n".to_string(),
        };

        // 2. 调用你附件中的 generate_response 逻辑
        match self.engine.generate_response(&prompt) {
            Ok(res) => res,
            Err(_) => "刀光一闪！".to_string(),
        }
    }
}
