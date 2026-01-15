use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug)]
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
    // 分类头
    w_out: Tensor,
    b_out: Tensor,
    // 新增：Pooler 层 (用于处理 CLS 向量)
    w_pooler: Option<Tensor>,
    b_pooler: Option<Tensor>,
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

        // 1. 加载 BERT
        let model = BertModel::load(vb.pp("bert"), &config)?;

        // 2. 尝试加载 Pooler 层 (通常在 bert.pooler 下)
        // 这一层非常关键，它把原始特征压缩到 -1~1 之间
        let w_pooler = vb
            .pp("bert")
            .get(
                (config.hidden_size, config.hidden_size),
                "pooler.dense.weight",
            )
            .ok();
        let b_pooler = vb
            .pp("bert")
            .get(config.hidden_size, "pooler.dense.bias")
            .ok();

        // 调试：打印一下是否找到了 pooler
        if w_pooler.is_some() {
            web_sys::console::log_1(&"Pooler layer loaded!".into());
        } else {
            web_sys::console::log_1(&"Warning: No Pooler layer found.".into());
        }

        // 3. 加载 Classifier (带兼容逻辑)
        let num_labels = 2;

        // 使用 or_else 链式尝试不同的 Key 名
        let w_out = vb
            .get((num_labels, config.hidden_size), "classifier.weight")
            .or_else(|_| {
                vb.get(
                    (num_labels, config.hidden_size),
                    "classifier.out_proj.weight",
                )
            })
            .or_else(|_| vb.get((num_labels, config.hidden_size), "classifier.dense.weight"))
            .map_err(|_| JsError::new("权重文件中缺少分类层 (classifier weight)"))?;

        let b_out = vb
            .get(num_labels, "classifier.bias")
            .or_else(|_| vb.get(num_labels, "classifier.out_proj.bias"))
            .or_else(|_| vb.get(num_labels, "classifier.dense.bias"))
            .map_err(|_| JsError::new("权重文件中缺少分类层偏置 (classifier bias)"))?;

        Ok(Self {
            model,
            tokenizer,
            w_out,
            b_out,
            w_pooler,
            b_pooler,
        })
    }

    pub fn predict(&self, text: &str) -> Result<SentiPulseResult, JsError> {
        let device = &Device::Cpu;
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // 调试 Token
        web_sys::console::log_1(&format!("Tokens: {:?}", tokens.get_ids()).into());

        let input_ids = Tensor::new(tokens.get_ids(), device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::new(tokens.get_type_ids(), device)?.unsqueeze(0)?;

        let enc = self.model.forward(&input_ids, &token_type_ids, None)?;

        // 取出 [CLS] (原始向量)
        let mut cls_token = enc.get(0)?.get(0)?.unsqueeze(0)?;

        // --- 核心修复：执行 Pooler (如果存在) ---
        if let (Some(w), Some(b)) = (&self.w_pooler, &self.b_pooler) {
            // Pooler 逻辑: Tanh( Linear(CLS) )
            cls_token = cls_token.matmul(&w.t()?)?.broadcast_add(b)?.tanh()?;
        }

        // 执行分类计算
        // logits 现在的形状是 [1, 3]
        let logits = cls_token
            .matmul(&self.w_out.t()?)?
            .broadcast_add(&self.b_out)?;

        // -----------------------------------------------------------
        // 🔧 修复核心：添加温度缩放 (Temperature Scaling)
        // 原始 logits 数值太小，导致 softmax 后的概率拉不开差距。
        // 我们手动乘以一个系数 (比如 5.0)，相当于降低 temperature，让结果更自信。
        // -----------------------------------------------------------
        let scale_factor = 1.0;
        let scaled_logits = (logits * scale_factor as f64)?;

        // 使用放大后的 logits 进行 Softmax
        let pr = candle_nn::ops::softmax(&scaled_logits.flatten_all()?, 0)?;
        let scores = pr.to_vec1::<f32>()?;

        // 自动适配二分类或三分类
        let (neg, pos, neu) = if scores.len() >= 3 {
            (scores[0], scores[1], scores[2])
        } else {
            let mut n = scores[0];
            let mut p = scores[1];
            let diff = (n - p).abs();

            // 基础中性分：当差距很大时，直接设为 0
            let mut m = if diff < 0.2 {
                0.8
            } else if diff < 0.4 {
                0.3
            } else {
                0.0 // 情绪明确，中性归零
            };

            // 执行归一化
            let total = n + p + m;
            if total > 0.0 {
                n = n / total;
                p = p / total;
                m = m / total;
                (n, p, m)
            } else {
                (0.33, 0.33, 0.34) // 兜底：防止除以零
            }
        };
        // 你会发现它们可能长这样：[0.1, 0.2, 0.5] -> 放大后 -> [0.5, 1.0, 2.5]
        web_sys::console::log_1(&format!("Raw Text: {}, Raw Scores: {:?}", text, scores).into());
        let result = SentiPulseResult {
            negative: neg,
            positive: pos, // 修正映射：实验证明开心时 index 1 最高
            neutral: neu,
        };
        // let result = SentiPulseResult {
        //     negative: scores[0],
        //     positive: scores[1], // 修正映射：实验证明开心时 index 1 最高
        //     neutral: scores[2],
        // };
        web_sys::console::log_1(&format!("Raw Text: {}, result: {:?}", text, result).into());

        Ok(result)
    }
}
