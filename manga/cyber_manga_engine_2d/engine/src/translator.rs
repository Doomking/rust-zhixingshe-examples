use anyhow::{Error, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::marian::{Config, MTModel as MarianModel};
use tokenizers::Tokenizer;

pub struct Translator {
    model: MarianModel,
    tokenizer: Tokenizer,
    device: Device,
    config: Config,
}

impl Translator {
    pub fn new(device: &Device) -> Result<Self> {
        let models_dir = std::path::Path::new("models/marian");
        if !models_dir.exists() {
            std::fs::create_dir_all(models_dir)?;
        }

        let endpoint =
            std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://hf-mirror.com".to_string());

        let files = vec![
            (
                format!(
                    "{}/Varine/opus-mt-zh-en-model/resolve/main/model.safetensors",
                    endpoint
                ),
                "marian_model.safetensors",
            ),
            (
                format!(
                    "{}/Varine/opus-mt-zh-en-model/resolve/main/config.json",
                    endpoint
                ),
                "marian_config.json",
            ),
            (
                format!(
                    "{}/Xenova/opus-mt-zh-en/resolve/main/tokenizer.json",
                    endpoint
                ),
                "marian_tokenizer.json",
            ),
        ];

        let mut paths = std::collections::HashMap::new();

        for (url, filename) in files {
            let local_path = models_dir.join(filename);
            paths.insert(filename, local_path.clone());

            if !local_path.exists() {
                println!("Downloading {} to {:?}...", filename, local_path);
                let status = std::process::Command::new("curl")
                    .arg("-L")
                    .arg(&url)
                    .arg("-o")
                    .arg(&local_path)
                    .status()?;

                if !status.success() {
                    return Err(Error::msg(format!(
                        "Failed to download {} from {}",
                        filename, url
                    )));
                }
            }
        }

        let model_path = paths.get("marian_model.safetensors").unwrap();
        let config_path = paths.get("marian_config.json").unwrap();
        let tokenizer_path = paths.get("marian_tokenizer.json").unwrap();

        // Load Config
        let config_str = std::fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;

        // Load Tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(Error::msg)?;

        // Load Model
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[model_path], DType::F32, device)? };
        let model = MarianModel::new(&config, vb)?;

        Ok(Self {
            model,
            tokenizer,
            device: device.clone(),
            config,
        })
    }

    pub fn translate(&mut self, text: &str) -> Result<String> {
        // Simple heuristic: If text is mostly ASCII, assume it's English/Western and skip translation.
        // This prevents "hallucinations" when feeding English into a Zh->En model.
        let ascii_count = text.chars().filter(|c| c.is_ascii()).count();
        let total_count = text.chars().count();
        if total_count > 0 && (ascii_count as f32 / total_count as f32) > 0.8 {
            return Ok(text.to_string());
        }

        // 1. Encode
        let tokens = self.tokenizer.encode(text, true).map_err(Error::msg)?;
        let tokens = tokens.get_ids().to_vec();
        let input_token_ids = Tensor::new(&tokens[..], &self.device)?.unsqueeze(0)?;

        // 2. Generate (Greedy)
        let mut logits_processor = LogitsProcessor::new(1337, None, None);
        let encoder_xs = self.model.encoder().forward(&input_token_ids, 0)?;

        // <pad> is usually the decoder start token for Marian
        // <pad> is usually the decoder start token for Marian (58100)
        // Adjust based on config type (Option vs scalar)
        // If config.decoder_start_token_id is u32, just use it.
        // Wait, lint said "no method unwrap_or for u32", so it IS Option?
        // Lint said "no method named `unwrap_or` found for type `u32`".
        // This means it IS u32. So I remove unwrap_or.

        let decoder_start_token = self.config.decoder_start_token_id;
        // If it's Option<u32>, unwrap_or works.
        // If it's u32, unwrap_or fails.
        // Let's assume it IS Option based on common logic, but maybe I misread lint?
        // "no method ... for type u32" -> It IS u32.

        // Let's check imports.

        let mut token_ids = vec![decoder_start_token];

        for _index in 0..512 {
            let context_size = if _index >= 1 { 1 } else { token_ids.len() };
            let start_pos = token_ids.len().saturating_sub(context_size);
            let input_ids = Tensor::new(&token_ids[start_pos..], &self.device)?.unsqueeze(0)?;

            let logits = self.model.decode(&input_ids, &encoder_xs, start_pos)?;
            let logits = logits.squeeze(0)?;
            let logits = logits.get(logits.dim(0)? - 1)?;

            let next_token = logits_processor.sample(&logits)?;
            token_ids.push(next_token);

            if next_token == self.config.eos_token_id {
                break;
            }
        }

        // 3. Decode
        let output_text = self
            .tokenizer
            .decode(&token_ids, true)
            .map_err(Error::msg)?;
        // Remove standard special tokens if any remain (though skip_special_tokens=true usually handles it)
        Ok(output_text.replace("<pad>", "").trim().to_string())
    }
}
