use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::qwen2::{Config, ModelForCausalLM};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

/// Structured panel data from LLM parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub background_visual: String,
    pub character_visual: String,
    pub mood: String,
    pub dialogues: Vec<Dialogue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dialogue {
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct LLMResponse {
    panels: Vec<Panel>,
}

/// AI-powered script parser using local LLM
pub struct ScriptAI {
    model: Arc<Mutex<ModelForCausalLM>>, // Arc<Mutex> for thread safety
    tokenizer: Tokenizer,
    device: Device,
}

impl ScriptAI {
    /// Initialize the LLM model (Qwen2-1.5B)
    pub fn new() -> Result<Self> {
        println!("🤖 Initializing Script AI (LLM)...");

        // Use Metal (GPU) on Mac
        let device = Device::new_metal(0)?;

        // Setup models directory
        let models_dir = std::path::Path::new("models/qwen2");
        if !models_dir.exists() {
            std::fs::create_dir_all(models_dir)?;
        }

        // Files to download (using HF mirror for faster download in China)
        let base_url = "https://hf-mirror.com/Qwen/Qwen2-1.5B-Instruct/resolve/main";
        let files = vec![
            ("config.json", format!("{}/config.json", base_url)),
            ("tokenizer.json", format!("{}/tokenizer.json", base_url)),
            (
                "model.safetensors",
                format!("{}/model.safetensors", base_url),
            ),
        ];

        println!("📦 Downloading Qwen2-1.5B model files...");

        let mut paths = std::collections::HashMap::new();
        for (filename, url) in files {
            let local_path = models_dir.join(filename);
            paths.insert(filename, local_path.clone());

            if !local_path.exists() {
                println!(
                    "  Downloading {} (~{})",
                    filename,
                    if filename.contains("safetensors") {
                        "1.5GB"
                    } else {
                        "<1MB"
                    }
                );

                let status = std::process::Command::new("curl")
                    .arg("-L")
                    .arg(&url)
                    .arg("-o")
                    .arg(&local_path)
                    .arg("--progress-bar")
                    .status()?;

                if !status.success() {
                    return Err(anyhow!("Failed to download {}", filename));
                }
            } else {
                println!("  ✓ {} already exists", filename);
            }
        }

        // Load config and tokenizer
        let config_path = paths.get("config.json").unwrap();
        let tokenizer_path = paths.get("tokenizer.json").unwrap();
        let weights_path = paths.get("model.safetensors").unwrap();

        println!("🔧 Loading model configuration...");
        let config: Config = serde_json::from_slice(&std::fs::read(config_path)?)?;
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| anyhow!("Tokenizer error: {}", e))?;

        // Load model weights
        println!("🔧 Loading model weights (~1.5GB)...");
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)?
        };

        let model = ModelForCausalLM::new(&config, vb)?;

        println!("✅ Script AI ready!");

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            tokenizer,
            device,
        })
    }

    /// Parse casual user input into structured panels
    pub fn parse_casual_text(&self, user_input: &str) -> Result<Vec<Panel>> {
        println!("🧠 Analyzing user input with LLM...");

        let system_prompt = r#"You are a professional manga storyboarder. Convert casual text into a Cinematic Storyboard JSON.

CRITICAL INSTRUCTIONS:
1. **Analyze the Scene**: Identify the setting (e.g., "old workshop"). Use this EXACT setting for ALL panels in the scene.
2. **Split Actions & Dialogue**:
   - Create separate panels for *actions* (silence) and *dialogues*.
   - NEVER lump multiple sentences into one panel.
   - Example Input: "She fixed the machine. Then she yelled 'I'm mad! It broke again!'"
   - Example Output:
     Panel 1: (Action) She fixing machine. (Silence)
     Panel 2: (Dialogue) "I'm mad!" (Angry face)
     Panel 3: (Dialogue) "It broke again!" (Despairing face)

3. **Visual Description (MUST BE ENGLISH)**:
   - **background_visual**: Describe ONLY the environment/setting. KEEP THIS IDENTICAL for all panels in the same scene.
     - Example: "in a cluttered old workshop, tools everywhere, rusty, steampunk vibe, sunlight streaming in, studio ghibli style"
   - **character_visual**: Describe ONLY the character pose and action.
     - Example: "1girl, 16yo, messy hair, holding a wrench, looking at smoke, angry expression, anime style"
   - DO NOT use Chinese in visuals.

4. **Dialogue (Original Language)**:
   - Keep "text" in the original language (Chinese).

JSON Format:
{
  "panels": [
    {
      "background_visual": "in old workshop, tools everywhere, rusty, steampunk vibe, sunlight streaming in, detailed background, studio ghibli style",
      "character_visual": "1girl, 16yo, messy hair, repairing mechanism, smoke rising, focused expression",
      "mood": "focused",
      "dialogues": [] 
    },
    {
      "background_visual": "in old workshop, tools everywhere, rusty, steampunk vibe, sunlight streaming in, detailed background, studio ghibli style",
      "character_visual": "1girl, 16yo, angry expression, shouting, clenched fists, smoke in background",
      "mood": "angry",
      "dialogues": [{"speaker": "Mio", "text": "气死我了！"}]
    }
  ]
}"#;

        let full_prompt = format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            system_prompt, user_input
        );

        // Generate LLM response
        let output = self.generate(&full_prompt, 512)?;

        println!("📝 LLM Output:\n{}", output);

        // Extract JSON from output
        self.extract_panels(&output)
    }

    /// Generate text using the LLM
    fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow!("Tokenization error: {}", e))?;

        let input_ids = Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?;
        let mut all_tokens = tokens.get_ids().to_vec();

        // First forward pass: process the entire prompt
        let mut model = self
            .model
            .lock()
            .map_err(|e| anyhow!("Mutex lock error: {}", e))?;

        let mut logits = model.forward(&input_ids, 0)?;
        drop(model);

        // Sample first token from the prompt logits
        let logits_squeezed = logits.squeeze(0)?.to_dtype(DType::F32)?;
        let last_logits = logits_squeezed.get(logits_squeezed.dim(0)? - 1)?;
        let mut next_token = self.sample_token(&last_logits)?;
        all_tokens.push(next_token);

        // Incremental generation: one token at a time
        for _ in 1..max_tokens {
            let next_token_tensor = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;

            let mut model = self
                .model
                .lock()
                .map_err(|e| anyhow!("Mutex lock error: {}", e))?;

            // seqlen_offset = number of tokens already processed (prompt + generated so far)
            logits = model.forward(&next_token_tensor, all_tokens.len() - 1)?;
            drop(model);

            let logits_squeezed = logits.squeeze(0)?.to_dtype(DType::F32)?;
            let last_logits = logits_squeezed.get(logits_squeezed.dim(0)? - 1)?;
            next_token = self.sample_token(&last_logits)?;

            all_tokens.push(next_token);

            // Check for EOS token (Qwen2 uses 151643 or 151645)
            if next_token == 151643 || next_token == 151645 {
                break;
            }
        }

        // Decode all tokens (skip the prompt tokens)
        let generated_tokens = &all_tokens[tokens.get_ids().len()..];
        let output = self
            .tokenizer
            .decode(generated_tokens, true)
            .map_err(|e| anyhow!("Decode error: {}", e))?;

        Ok(output)
    }

    /// Sample next token from logits (greedy sampling for now)
    fn sample_token(&self, logits: &Tensor) -> Result<u32> {
        let logits_vec = logits.to_vec1::<f32>()?;
        let max_idx = logits_vec
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(idx, _)| idx)
            .ok_or_else(|| anyhow!("Failed to find max logit"))?;
        Ok(max_idx as u32)
    }

    /// Extract JSON from LLM output with multiple fallback strategies
    fn extract_panels(&self, text: &str) -> Result<Vec<Panel>> {
        // Strategy 1: Direct parse
        if let Ok(response) = serde_json::from_str::<LLMResponse>(text) {
            return Ok(response.panels);
        }

        // Strategy 2: Extract JSON block (look for {})
        if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                let json_str = &text[start..=end];
                if let Ok(response) = serde_json::from_str::<LLMResponse>(json_str) {
                    return Ok(response.panels);
                }
            }
        }

        // Strategy 3: Extract from markdown code block
        if let Some(start) = text.find("```json") {
            if let Some(end) = text[start..].find("```") {
                let json_str = &text[start + 7..start + end].trim();
                if let Ok(response) = serde_json::from_str::<LLMResponse>(json_str) {
                    return Ok(response.panels);
                }
            }
        }

        // Strategy 4: Look for "panels" key
        if let Some(panels_pos) = text.find("\"panels\"") {
            let search_start = text[..panels_pos].rfind('{').unwrap_or(0);
            let remaining = &text[search_start..];
            if let Some(end) = remaining.rfind('}') {
                let json_str = &remaining[..=end];
                if let Ok(response) = serde_json::from_str::<LLMResponse>(json_str) {
                    return Ok(response.panels);
                }
            }
        }

        Err(anyhow!(
            "Failed to extract valid JSON from LLM output:\n{}",
            text
        ))
    }
}
