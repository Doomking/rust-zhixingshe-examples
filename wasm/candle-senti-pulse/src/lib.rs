use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct SentiPulseResult {
    negative: f32,
    positive: f32,
    neutral: f32,
}

#[wasm_bindgen]
impl SentiPulseResult {
    #[wasm_bindgen(getter)]
    pub fn negative(&self) -> f32 {
        self.negative
    }
    #[wasm_bindgen(getter)]
    pub fn positive(&self) -> f32 {
        self.positive
    }
    #[wasm_bindgen(getter)]
    pub fn neutral(&self) -> f32 {
        self.neutral
    }
}

#[wasm_bindgen]
pub struct SentiPulseEngine {
    model: BertModel,
    tokenizer: Tokenizer,
    // 新增：手动持有分类层的权重
    w_out: Tensor,
    b_out: Tensor,
}

#[wasm_bindgen]
impl SentiPulseEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(
        weights: &[u8],
        tokenizer_data: &[u8],
        config_str: &str,
    ) -> Result<SentiPulseEngine, JsError> {
        console_error_panic_hook::set_once();
        let device = &Device::Cpu;

        let tokenizer =
            Tokenizer::from_bytes(tokenizer_data).map_err(|e| JsError::new(&e.to_string()))?;
        let config: Config =
            serde_json::from_str(config_str).map_err(|e| JsError::new(&e.to_string()))?;

        let vb = VarBuilder::from_buffered_safetensors(weights.to_vec(), DType::F32, device)?;

        // 1. 加载基础 BERT (注意：通常权重在 "bert" 命名空间下)
        let model = BertModel::load(vb.pp("bert"), &config)?;

        // 2. 核心修复：直接使用 3 (三分类)
        let num_labels = 3;

        // 加载分类层权重
        // 注意：如果 vb.pp("bert") 已经进入了前缀，这里可能需要回到根节点获取 classifier
        // 或者尝试 vb.get(...)
        let w_out = vb
            .get((num_labels, config.hidden_size), "classifier.weight")
            .map_err(|_| JsError::new("找不到 classifier.weight，请检查权重键名"))?;
        let b_out = vb
            .get(num_labels, "classifier.bias")
            .map_err(|_| JsError::new("找不到 classifier.bias"))?;

        Ok(Self {
            model,
            tokenizer,
            w_out,
            b_out,
        })
    }

    pub fn predict(&self, text: &str) -> Result<SentiPulseResult, JsError> {
        let device = &Device::Cpu;
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let input_ids = Tensor::new(tokens.get_ids(), device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::new(tokens.get_type_ids(), device)?.unsqueeze(0)?;

        // 1. 运行 BERT 得到 [1, seq_len, 768]
        let enc = self.model.forward(&input_ids, &token_type_ids, None)?;

        // 2. 提取 [CLS] 向量 (第0个token) -> 形状 [1, 768]
        let cls_token = enc.get(0)?.get(0)?.unsqueeze(0)?;

        // 3. 手动执行分类层计算: Logits = CLS * W^T + b -> 形状 [1, 3]
        let logits = cls_token
            .matmul(&self.w_out.t()?)?
            .broadcast_add(&self.b_out)?;

        // 4. Softmax 归一化并转为 Vec
        let pr = candle_nn::ops::softmax(&logits.flatten_all()?, 0)?;
        let scores = pr.to_vec1::<f32>()?;

        Ok(SentiPulseResult {
            negative: scores[0],
            neutral: scores[1],
            positive: scores[2],
        })
    }
}
