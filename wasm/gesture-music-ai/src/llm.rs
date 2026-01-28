use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen2::{Config as QwenConfig, ModelForCausalLM as QwenModel};
use tokenizers::Tokenizer;

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
    /// 核心初始化：直接从字节流加载
    pub fn init(weights_data: &[u8], tokenizer_data: &[u8], config_data: &[u8]) -> Result<Self> {
        let device = Device::Cpu;

        // 1. 加载分词器
        let tokenizer = Tokenizer::from_bytes(tokenizer_data)
            .map_err(anyhow::Error::msg)
            .context("Tokenizer 加载失败")?;

        // 2. 解析配置
        let config: QwenConfig =
            serde_json::from_slice(config_data).context("Qwen 配置 JSON 解析失败")?;

        // 3. 构建权重加载器
        let vb = VarBuilder::from_buffered_safetensors(weights_data.to_vec(), DType::F32, &device)
            .context("Safetensors 权重读取失败")?;

        // 4. 实例化模型
        let model = QwenModel::new(&config, vb).context("Qwen 模型结构初始化失败")?;

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

    /// 准备推理：重置状态并对 Prompt 编码
    pub fn apply_prompt(&mut self, prompt: &str) -> Result<()> {
        self.reset();

        let tokens = self
            .tokenizer
            .encode(prompt, false)
            .map_err(anyhow::Error::msg)?;

        self.token_ids = tokens.get_ids().to_vec();

        // 预解码 Prompt，确定起始偏移量
        let decoded = self
            .tokenizer
            .decode(&self.token_ids, false)
            .map_err(anyhow::Error::msg)?;

        self.last_decoded_len = decoded.len();
        self.is_finished = false;
        Ok(())
    }

    /// 推理步进：生成下一个 Token
    pub fn step(&mut self) -> Result<String> {
        if self.is_finished || self.token_ids.is_empty() {
            return Ok(String::new());
        }

        let eos_id = 151643;
        let im_start_id = 151644;
        let im_end_id = 151645;

        // 确定上下文窗口
        let context_size = if self.pos == 0 {
            self.token_ids.len()
        } else {
            1
        };
        let start_idx = self.token_ids.len() - context_size;

        let input_tensor = Tensor::new(&self.token_ids[start_idx..], &self.device)?.unsqueeze(0)?;

        // 前向传播
        let logits = self.model.forward(&input_tensor, self.pos)?;
        self.pos += context_size;

        // 采样逻辑 (Temperature 0.7)
        let logits = (logits / 0.7)?.flatten_all()?;

        // 重复惩罚 (Penalty 1.1)
        let mut logits_vec = logits.to_vec1::<f32>()?;
        for &id in &self.token_ids {
            let idx = id as usize;
            if idx < logits_vec.len() {
                if logits_vec[idx] > 0.0 {
                    logits_vec[idx] /= 1.1;
                } else {
                    logits_vec[idx] *= 1.1;
                }
            }
        }

        let logits = Tensor::from_vec(logits_vec, logits.dims(), &self.device)?;
        let next_token_id = logits.argmax(0)?.to_scalar::<u32>()?;

        // 终止符判断
        if next_token_id == eos_id || next_token_id == im_start_id || next_token_id == im_end_id {
            self.is_finished = true;
            return Ok(String::new());
        }

        self.token_ids.push(next_token_id);

        // 增量解码文字
        let all_text = self
            .tokenizer
            .decode(&self.token_ids, false)
            .map_err(anyhow::Error::msg)?;

        let new_text = &all_text[self.last_decoded_len..];

        // 解决 UTF-8 截断导致的乱码占位符问题
        if new_text.ends_with('\u{FFFD}') {
            Ok(String::new())
        } else {
            self.last_decoded_len = all_text.len();
            Ok(new_text.to_string())
        }
    }

    /// 一次性生成完整响应
    pub fn generate_response(&mut self, prompt: &str) -> Result<String> {
        self.apply_prompt(prompt)?;
        let mut text = String::new();
        // 限制最大长度防止死循环
        for _ in 0..64 {
            let piece = self.step()?;
            if self.is_finished {
                break;
            }
            text.push_str(&piece);
        }
        Ok(text.trim().to_string())
    }

    pub fn reset(&mut self) {
        self.model.clear_kv_cache();
        self.pos = 0;
        self.token_ids.clear();
        self.last_decoded_len = 0;
        self.is_finished = false;
    }
}
