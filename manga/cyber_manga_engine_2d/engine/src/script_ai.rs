use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::qwen2::{Config, ModelForCausalLM};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

/// Structured panel data for the rest of the engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub background_visual: String,
    pub character_visual: String,
    pub mood: String,
    pub dialogues: Vec<Dialogue>,
    /// Pre-generated English SD prompt (skips translation step)
    #[serde(default)]
    pub sd_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dialogue {
    pub speaker: String,
    pub text: String,
}

/// Intermediate struct for parsing LLM response (New Format)
#[derive(Debug, Deserialize)]
struct RawLLMResponse {
    #[serde(default)]
    characters: Option<serde_json::Value>, // Optional and loose to prevent crash
    panels: Vec<RawPanel>,
}

#[derive(Debug, Deserialize)]
struct RawPanel {
    background: String, // New key
    character: String,  // Name
    action: String,     // Pose/Expression
    mood: String,
    #[serde(default)]
    dialogues: Vec<Dialogue>,
}

/// AI-powered script parser using local LLM
pub struct ScriptAI {
    model: Arc<Mutex<ModelForCausalLM>>, // Arc<Mutex> for thread safety
    tokenizer: Tokenizer,
    device: Device,
}

impl ScriptAI {
    /// Initialize the LLM model (Qwen2.5-1.5B-Instruct for quality)
    pub fn new(device: &Device) -> Result<Self> {
        println!("🤖 Initializing Script AI (Qwen2.5-1.5B-Instruct)...");

        let device = device.clone();

        // Setup models directory
        let models_dir = std::path::Path::new("models/qwen2.5-1.5b");
        if !models_dir.exists() {
            std::fs::create_dir_all(models_dir)?;
        }

        // Qwen2.5-1.5B-Instruct: 3x larger than 0.5B, much better instruction following
        let base_url = "https://hf-mirror.com/Qwen/Qwen2.5-1.5B-Instruct/resolve/main";
        let files = vec![
            ("config.json", format!("{}/config.json", base_url)),
            ("tokenizer.json", format!("{}/tokenizer.json", base_url)),
            (
                "model.safetensors",
                format!("{}/model.safetensors", base_url),
            ),
        ];

        println!("📦 Downloading Qwen2.5-1.5B model files...");

        let mut paths = std::collections::HashMap::new();
        for (filename, url) in files {
            let local_path = models_dir.join(filename);
            paths.insert(filename, local_path.clone());

            if !local_path.exists() {
                println!(
                    "  Downloading {} (~{})",
                    filename,
                    if filename.contains("safetensors") {
                        "3GB"
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

        println!("🔧 Loading model weights (~3GB)...");
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

    /// Parse user casual text into structured panels
    /// Uses LLM for extraction & translation (General Purpose), Programmatic for Structure (Reliable)
    pub fn parse_casual_text(&self, user_input: &str) -> Result<Vec<Panel>> {
        println!("🧠 Analyzing user input (Hybrid Approach)...");

        // 1. Build panels programmatically (Structure is reliable)
        let mut panels = self.build_panels_programmatic(user_input)?;

        // 2. Extract Story Elements via LLM (Content Understanding)
        // We use LLM to extract Chinese keywords because Regex is brittle for general inputs
        let (scene_cn, char_cn, mood_cn) = self.extract_story_info(user_input);
        println!(
            "📝 XML Extracted: Scene='{}', Char='{}', Mood='{}'",
            scene_cn, char_cn, mood_cn
        );

        // 3. Translate to English SD Tags via LLM (Visual Translation)
        // We use a specific prompt for tags to avoid "sentence translation" issues
        let scene_en = self.translate_to_tags(&scene_cn, "background/scenery");
        let char_en =
            self.translate_to_tags(&char_cn, "character appearance, e.g. 1girl, 1boy, clothing");

        println!("🔤 Scene Tags: {} -> {}", scene_cn, scene_en);
        println!("🔤 Char Tags:  {} -> {}", char_cn, char_en);

        // 4. Update Panels with LLM-derived data
        // Set sd_prompt for all panels with consistent character + background
        let mood_tags = [
            "calm, peaceful atmosphere",
            "focused, working, busy",
            "tension, problem, dramatic",
            "shouting, emotional, climax",
        ];

        for (i, panel) in panels.iter_mut().enumerate() {
            // Update visual descriptions with extracted Chinese text
            panel.background_visual = scene_cn.clone();
            panel.character_visual = char_cn.clone();
            panel.mood = mood_cn.clone();

            // Set SD Prompt
            let mood_tag = if i < mood_tags.len() {
                mood_tags[i]
            } else {
                "neutral"
            };

            panel.sd_prompt = Some(format!("{}, {}, {}", char_en, scene_en, mood_tag));
        }

        // 5. Ensure dialogue is ONLY on the last panel (Programmatic Fix)
        let character_name = Self::extract_character_name(user_input);
        let real_dialogues = Self::extract_dialogues(user_input, &character_name);

        // Clear ALL dialogues first
        for panel in &mut panels {
            panel.dialogues.clear();
        }
        // Distribute real dialogues across panels (Smart Placement)
        let num_dialogues = real_dialogues.len();
        let num_panels = panels.len();

        if num_dialogues > 0 {
            // Strategy: Map M dialogues to N panels
            // 1 dialogue -> Last panel (Conclusion)
            // 2 dialogues -> First (Setup) & Last (Conclusion)
            // 3 dialogues -> First, Third (Twist), Last
            // 4+ dialogues -> Sequential

            let target_indices: Vec<usize> = match num_dialogues {
                1 => vec![num_panels - 1],
                2 => vec![0, num_panels - 1],
                3 => vec![0, 2.min(num_panels - 1), num_panels - 1],
                _ => (0..num_panels.min(num_dialogues)).collect(),
            };

            for (i, &panel_idx) in target_indices.iter().enumerate() {
                if let Some(text) = real_dialogues.get(i) {
                    if panel_idx < num_panels {
                        panels[panel_idx].dialogues.push(Dialogue {
                            speaker: character_name.clone(),
                            text: text.clone(),
                        });
                    }
                }
            }
        }

        Ok(panels)
    }

    /// Extract story info using LLM (Chinese -> Chinese)
    /// Returns (Scene, Character, Mood)
    fn extract_story_info(&self, text: &str) -> (String, String, String) {
        let prompt = format!(
            "<|im_start|>system\nAnalyze the story. Extract visual keywords in Chinese. Format: JSON.\nTarget keys:\n- scene (visual description ONLY, NO dialogue/speech)\n- character (appearance/clothing ONLY, NO dialogue)\n- mood (emotional state)\n<|im_end|>\n<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            text
        );

        match self.generate_greedy(&prompt, 512) {
            Ok(output) => {
                // Simple JSON parser or Regex extraction would be better here to avoid parsing issues
                // For robustness, let's use a simple line-based parsing if JSON fails,
                // but since we asked for JSON, let's try to find the JSON block.
                let json_start = output.find('{').unwrap_or(0);
                let json_end = output.rfind('}').map(|p| p + 1).unwrap_or(output.len());
                let json_str = &output[json_start..json_end];

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let scene = val["scene"].as_str().unwrap_or("室内").to_string();
                    let character = val["character"].as_str().unwrap_or("人物").to_string();
                    let mood = val["mood"].as_str().unwrap_or("平静").to_string();
                    return (scene, character, mood);
                }
            }
            Err(_) => {}
        }

        // Fallback if LLM fails
        (
            Self::extract_scene(text),
            Self::extract_character_visual(text, "主角"),
            "平静".to_string(),
        )
    }

    /// Translate Chinese description to English SD Tags
    fn translate_to_tags(&self, text: &str, context: &str) -> String {
        // 1. LLM Translation
        let prompt = format!(
            "<|im_start|>system\nRole: Translator.\nTask: Convert Chinese keywords to English Stable Diffusion tags.\nContext: {}.\nRules:\n1. Comma separated keywords only.\n2. NO sentences.\n3. ABSOLUTELY NO dialogue or speech text.\nExample input: '破旧工坊，大喊'\nExample output: 'old workshop, rustic, tools, shouting, open mouth'\n<|im_end|>\n<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            context, text
        );

        let llm_output = match self.generate_greedy(&prompt, 128) {
            Ok(output) => output
                .chars()
                .filter(|c| c.is_ascii() || c.is_whitespace() || *c == ',')
                .collect::<String>()
                .trim()
                .to_string(),
            Err(_) => "quality, masterpiece".to_string(),
        };

        // 2. Dictionary Reinforcement (Hybrid Fix for Hallucinations)
        // If context is "background/scenery", we check for known scene keywords in the original Chinese text
        // and FORCE them into the English output.
        let dict_tags = if context.contains("background") {
            Self::cn_to_en_scene(text)
        } else if context.contains("character") {
            Self::cn_to_en_character(text)
        } else {
            String::new()
        };

        // Combine: Dictionary (Reliable) + LLM (Creative)
        if dict_tags.is_empty() {
            llm_output
        } else {
            format!("{}, {}", dict_tags, llm_output)
        }
    }

    /// Programmatic fallback: build 4-panel structure from text extraction
    fn build_panels_programmatic(&self, user_input: &str) -> Result<Vec<Panel>> {
        let character_name = Self::extract_character_name(user_input);
        let character_visual = Self::extract_character_visual(user_input, &character_name);
        let dialogues = Self::extract_dialogues(user_input, &character_name);
        let scene = Self::extract_scene(user_input);
        let mood = Self::extract_mood(user_input);

        println!(
            "  � {} | 🏠 {} | 😤 {} | 💬 {:?}",
            character_name, scene, mood, dialogues
        );

        let panels = vec![
            Panel {
                background_visual: format!("{}，平静的氛围", scene),
                character_visual: character_visual.clone(),
                mood: "平静".to_string(),
                dialogues: if dialogues.len() > 1 {
                    vec![Dialogue {
                        speaker: character_name.clone(),
                        text: dialogues[0].clone(),
                    }]
                } else {
                    vec![]
                },
                sd_prompt: None,
            },
            Panel {
                background_visual: format!("{}，正在忙碌工作", scene),
                character_visual: character_visual.clone(),
                mood: "专注".to_string(),
                dialogues: vec![],
                sd_prompt: None,
            },
            Panel {
                background_visual: format!("{}，出了问题，气氛紧张", scene),
                character_visual: character_visual.clone(),
                mood: mood.clone(),
                dialogues: vec![],
                sd_prompt: None,
            },
            Panel {
                background_visual: format!("{}，{}", scene, mood),
                character_visual: character_visual.clone(),
                mood: mood,
                dialogues: vec![Dialogue {
                    speaker: character_name,
                    text: dialogues.last().cloned().unwrap_or("……".to_string()),
                }],
                sd_prompt: None,
            },
        ];

        println!("✅ Built 4-panel storyboard (programmatic fallback)");
        Ok(panels)
    }

    /// Extract character name from Chinese text
    fn extract_character_name(text: &str) -> String {
        // Pattern: 叫X的  or  叫X，
        if let Some(pos) = text.find('叫') {
            let after = &text[pos + '叫'.len_utf8()..];
            // Find the end of the name (next particle: 的, ，, 。, etc.)
            let end = after
                .find(|c: char| "的，。！？ ".contains(c))
                .unwrap_or(after.len().min(12));
            let name = after[..end].trim();
            if !name.is_empty() && name.chars().count() <= 4 {
                return name.to_string();
            }
        }
        // Fallback: look for common name patterns
        "主角".to_string()
    }

    /// Extract character visual description
    fn extract_character_visual(text: &str, name: &str) -> String {
        let mut parts = Vec::new();

        // Extract age
        for pattern in &["岁", "年纪"] {
            if let Some(pos) = text.find(pattern) {
                let before = &text[..pos];
                // Find start of the number (scan backwards for non-digit)
                let start = before
                    .rfind(|c: char| !c.is_ascii_digit())
                    .map(|p| {
                        // We found a non-digit at p. Start of number is after this char.
                        let char_len = before[p..]
                            .chars()
                            .next()
                            .map(|c| c.len_utf8())
                            .unwrap_or(1);
                        p + char_len
                    })
                    .unwrap_or(0);

                let num_str = &before[start..];
                if !num_str.is_empty() {
                    parts.push(format!("{}{}", num_str, pattern));
                }
            }
        }

        // Extract clothing/appearance keywords
        let appearance_keywords = [
            "穿",
            "戴",
            "拿",
            "蓝色",
            "红色",
            "白色",
            "黑色",
            "长发",
            "短发",
            "工作服",
            "裙子",
            "围裙",
        ];
        for kw in &appearance_keywords {
            if text.contains(kw) {
                // Find the sentence containing this keyword
                for sentence in text.split(|c: char| "。，！？".contains(c)) {
                    if sentence.contains(kw) && !sentence.contains("说") && !sentence.contains("喊")
                    {
                        parts.push(sentence.trim().to_string());
                        break;
                    }
                }
            }
        }

        if parts.is_empty() {
            format!("{}，女孩", name)
        } else {
            parts.join("，")
        }
    }

    /// Extract dialogues from quoted text
    fn extract_dialogues(text: &str, character_name: &str) -> Vec<String> {
        let mut dialogues = Vec::new();

        // Find text in Chinese quotes: "..." or 「...」 or "..."
        let quote_pairs = [
            ('\u{201C}', '\u{201D}'),
            ('\u{300C}', '\u{300D}'),
            ('"', '"'),
        ];

        for (open, close) in &quote_pairs {
            let mut remaining = text;
            while let Some(start) = remaining.find(*open) {
                let after_open = &remaining[start + open.len_utf8()..];
                if let Some(end) = after_open.find(*close) {
                    let dialogue = &after_open[..end];
                    if !dialogue.is_empty() {
                        dialogues.push(dialogue.to_string());
                    }
                    remaining = &after_open[end + close.len_utf8()..];
                } else {
                    break;
                }
            }
        }

        // If no quoted text found, try to find dialogue markers
        if dialogues.is_empty() {
            for sentence in text.split(|c: char| "。！？".contains(c)) {
                let s = sentence.trim();
                if (s.contains("说") || s.contains("喊") || s.contains("叫")) && s.len() > 6 {
                    // Extract the speech part after 说/喊
                    if let Some(pos) = s.find("说").or(s.find("喊")) {
                        let speech = &s[pos + 3..]; // Skip the verb (3 bytes for Chinese char)
                        let speech = speech.trim_start_matches(|c: char| "：:\"'\"".contains(c));
                        if !speech.is_empty() {
                            dialogues.push(speech.to_string());
                        }
                    }
                }
            }
        }

        let _ = character_name; // Used by caller
        dialogues
    }

    /// Extract scene/setting description
    fn extract_scene(text: &str) -> String {
        let scene_keywords = [
            "工坊", "学校", "教室", "房间", "街道", "森林", "海边", "山上", "城市", "家", "厨房",
            "花园", "商店", "医院",
        ];

        for kw in &scene_keywords {
            if text.contains(kw) {
                return kw.to_string();
            }
        }

        // Try to find location from context
        for sentence in text.split(|c: char| "。，！？".contains(c)) {
            let s = sentence.trim();
            if s.contains("在") && s.len() < 30 {
                return s.to_string();
            }
        }

        "室内".to_string()
    }

    /// Chinese→English scene dictionary for SD prompts (100% reliable)
    fn cn_to_en_scene(scene_cn: &str) -> String {
        let mut tags = Vec::new();

        let scene_dict: Vec<(&str, &str)> = vec![
            (
                "工坊",
                "old workshop, indoor, wooden shelves, tools, workbench",
            ),
            ("学校", "school building, campus, outdoor"),
            ("教室", "classroom, indoor, desks, blackboard"),
            ("房间", "room, indoor, furniture"),
            ("街道", "street, outdoor, buildings"),
            ("森林", "forest, trees, nature, outdoor"),
            ("海边", "beach, ocean, waves, outdoor"),
            ("山上", "mountain, outdoor, scenic"),
            ("城市", "city, urban, buildings, outdoor"),
            ("家", "home, indoor, cozy"),
            ("厨房", "kitchen, indoor, cooking"),
            ("花园", "garden, flowers, outdoor"),
            ("商店", "shop, store, indoor"),
            ("医院", "hospital, indoor, medical"),
            ("室内", "indoor, room"),
        ];

        for (cn, en) in &scene_dict {
            if scene_cn.contains(cn) {
                tags.push(en.to_string());
                break;
            }
        }

        // Weather/atmosphere keywords
        if scene_cn.contains("阳光") || scene_cn.contains("晴") {
            tags.push("sunlight, warm lighting".to_string());
        }
        if scene_cn.contains("破旧") || scene_cn.contains("老旧") {
            tags.push("old, rustic, worn".to_string());
        }

        if tags.is_empty() {
            "indoor scene".to_string()
        } else {
            tags.join(", ")
        }
    }

    /// Chinese→English character dictionary for SD prompts (100% reliable)
    fn cn_to_en_character(char_cn: &str) -> String {
        let mut tags = Vec::new();

        // Gender
        if char_cn.contains("女") || char_cn.contains("她") || char_cn.contains("姑娘") {
            tags.push("1girl".to_string());
        } else if char_cn.contains("男") || char_cn.contains("他") || char_cn.contains("小伙") {
            tags.push("1boy".to_string());
        } else {
            tags.push("1girl".to_string()); // Default
        }

        // Age
        let age_patterns = [
            ("16岁", "16yo"),
            ("15岁", "15yo"),
            ("14岁", "14yo"),
            ("17岁", "17yo"),
            ("18岁", "18yo"),
            ("20岁", "20yo"),
            ("少女", "teenage girl"),
            ("少年", "teenage boy"),
        ];
        for (cn, en) in &age_patterns {
            if char_cn.contains(cn) {
                tags.push(en.to_string());
                break;
            }
        }

        // Hair
        let hair_dict = [
            ("长发", "long hair"),
            ("短发", "short hair"),
            ("马尾", "ponytail"),
            ("双马尾", "twintails"),
            ("辫子", "braid"),
            ("红发", "red hair"),
            ("黑发", "black hair"),
            ("金发", "blonde hair"),
            ("棕发", "brown hair"),
            ("白发", "white hair"),
        ];
        let mut has_hair = false;
        for (cn, en) in &hair_dict {
            if char_cn.contains(cn) {
                tags.push(en.to_string());
                has_hair = true;
            }
        }
        if !has_hair {
            tags.push("brown hair".to_string()); // Default for anime
        }

        // Clothing
        let clothes_dict = [
            ("工作服", "work clothes, overalls"),
            ("裙子", "skirt"),
            ("围裙", "apron"),
            ("校服", "school uniform"),
            ("制服", "uniform"),
            ("蓝色", "blue clothes"),
            ("红色", "red clothes"),
            ("白色", "white clothes"),
            ("黑色", "black clothes"),
        ];
        for (cn, en) in &clothes_dict {
            if char_cn.contains(cn) {
                tags.push(en.to_string());
            }
        }

        tags.join(", ")
    }

    /// Extract mood from text
    fn extract_mood(text: &str) -> String {
        let mood_map = [
            ("生气", "愤怒"),
            ("气死", "愤怒"),
            ("愤怒", "愤怒"),
            ("开心", "快乐"),
            ("高兴", "快乐"),
            ("笑", "快乐"),
            ("难过", "悲伤"),
            ("哭", "悲伤"),
            ("伤心", "悲伤"),
            ("害怕", "恐惧"),
            ("紧张", "紧张"),
            ("惊讶", "惊讶"),
            ("突然", "惊讶"),
        ];

        for (keyword, mood) in &mood_map {
            if text.contains(keyword) {
                return mood.to_string();
            }
        }

        "平静".to_string()
    }

    fn extract_panels(&self, output: &str) -> Result<Vec<Panel>> {
        // Find JSON content
        let start = output.find('{').unwrap_or(0);
        let end = output.rfind('}').unwrap_or(output.len()) + 1;
        let json_str = &output[start..end];

        println!("🔍 Attempting to parse JSON (New Format)...");

        // Try parsing as RawLLMResponse (New Format)
        match serde_json::from_str::<RawLLMResponse>(json_str) {
            Ok(raw_response) => {
                println!("✅ Parsed New Format Successfully");
                let mut panels = Vec::new();

                for raw_panel in raw_response.panels {
                    // Merge global character visual with local action
                    let mut char_base = "1girl".to_string(); // Default

                    // Try to extract character visual from the loose JSON
                    if let Some(chars_val) = &raw_response.characters {
                        if let Some(chars_obj) = chars_val.as_object() {
                            if let Some(val) = chars_obj.get(&raw_panel.character) {
                                if let Some(s) = val.as_str() {
                                    char_base = s.to_string();
                                }
                            }
                        }
                    }

                    let full_character_visual = format!("{}, {}", char_base, raw_panel.action);

                    panels.push(Panel {
                        background_visual: raw_panel.background,
                        character_visual: full_character_visual,
                        mood: raw_panel.mood,
                        dialogues: raw_panel.dialogues,
                        sd_prompt: None,
                    });
                }

                // Ensure exactly 4 panels if possible? Editor handles 2/3/4/n.
                // Constraint in prompt should handle it.
                Ok(panels)
            }
            Err(e) => {
                println!("⚠️ Failed to parse New Format: {}", e);
                println!("🔄 Attempting Legacy Fallback...");

                // Fallback: Try parsing keys 'background_visual' etc. if LLM used old format by mistake
                #[derive(Deserialize)]
                struct LegacyPanel {
                    background_visual: String,
                    character_visual: String,
                    mood: String,
                    dialogues: Vec<Dialogue>,
                    #[serde(default)]
                    sd_prompt: Option<String>,
                }
                #[derive(Deserialize)]
                struct LegacyResponse {
                    panels: Vec<LegacyPanel>,
                }

                let legacy_response: LegacyResponse =
                    serde_json::from_str(json_str).map_err(|e2| {
                        anyhow!(
                            "Failed to parse JSON (New & Legacy): {}\nOriginal Error: {}",
                            e2,
                            e
                        )
                    })?;

                println!("✅ Parsed Legacy Format");

                let panels = legacy_response
                    .panels
                    .into_iter()
                    .map(|p| Panel {
                        background_visual: p.background_visual,
                        character_visual: p.character_visual,
                        mood: p.mood,
                        dialogues: p.dialogues,
                        sd_prompt: p.sd_prompt,
                    })
                    .collect();

                Ok(panels)
            }
        }
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

        model.clear_kv_cache();
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

    /// Translate text to English using the LLM (greedy for determinism)
    pub fn translate(&self, text: &str) -> Result<String> {
        let prompt = format!(
            "<|im_start|>system\nYou are a professional translator. Translate the following Chinese text to English for an image generation prompt. Keep it concise, descriptive, and focus on visual elements.\nText: {}\nTranslation:<|im_end|>\n<|im_start|>assistant\n",
            text
        );
        self.generate_greedy(&prompt, 128)
    }

    /// Generate text with greedy sampling (for translation determinism)
    fn generate_greedy(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow!("Tokenization error: {}", e))?;

        let input_ids = Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?;
        let mut all_tokens = tokens.get_ids().to_vec();

        let mut model = self
            .model
            .lock()
            .map_err(|e| anyhow!("Mutex lock error: {}", e))?;

        model.clear_kv_cache();
        let mut logits = model.forward(&input_ids, 0)?;
        drop(model);

        let logits_squeezed = logits.squeeze(0)?.to_dtype(DType::F32)?;
        let last_logits = logits_squeezed.get(logits_squeezed.dim(0)? - 1)?;
        let mut next_token = self.sample_token_greedy(&last_logits)?;
        all_tokens.push(next_token);

        for _ in 1..max_tokens {
            let next_token_tensor = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;

            let mut model = self
                .model
                .lock()
                .map_err(|e| anyhow!("Mutex lock error: {}", e))?;

            logits = model.forward(&next_token_tensor, all_tokens.len() - 1)?;
            drop(model);

            let logits_squeezed = logits.squeeze(0)?.to_dtype(DType::F32)?;
            let last_logits = logits_squeezed.get(logits_squeezed.dim(0)? - 1)?;
            next_token = self.sample_token_greedy(&last_logits)?;

            all_tokens.push(next_token);

            if next_token == 151643 || next_token == 151645 {
                break;
            }
        }

        let generated_tokens = &all_tokens[tokens.get_ids().len()..];
        let output = self
            .tokenizer
            .decode(generated_tokens, true)
            .map_err(|e| anyhow!("Decode error: {}", e))?;

        Ok(output)
    }

    fn sample_token(&self, logits: &Tensor) -> Result<u32> {
        self.sample_token_with_temp(logits, 0.7)
    }

    fn sample_token_greedy(&self, logits: &Tensor) -> Result<u32> {
        let logits_v: Vec<f32> = logits.to_vec1()?;
        let max_id = logits_v
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(index, _)| index)
            .unwrap();
        Ok(max_id as u32)
    }

    fn sample_token_with_temp(&self, logits: &Tensor, temperature: f64) -> Result<u32> {
        use rand::Rng;

        if temperature <= 0.0 {
            return self.sample_token_greedy(logits);
        }

        let logits_v: Vec<f32> = logits.to_vec1()?;
        let top_p = 0.9_f32;

        // Apply temperature
        let scaled: Vec<f32> = logits_v.iter().map(|&x| x / temperature as f32).collect();

        // Softmax
        let max_val = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = scaled.iter().map(|&x| (x - max_val).exp()).collect();
        let sum: f32 = exps.iter().sum();
        let probs: Vec<f32> = exps.iter().map(|&x| x / sum).collect();

        // Top-p (nucleus) sampling
        let mut indexed: Vec<(usize, f32)> = probs.iter().cloned().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let mut cumulative = 0.0_f32;
        let mut candidates: Vec<(usize, f32)> = Vec::new();
        for (idx, p) in &indexed {
            cumulative += p;
            candidates.push((*idx, *p));
            if cumulative >= top_p {
                break;
            }
        }

        // Re-normalize candidate probabilities
        let cand_sum: f32 = candidates.iter().map(|(_, p)| p).sum();
        let mut rng = rand::thread_rng();
        let r: f32 = rng.gen::<f32>() * cand_sum;

        let mut acc = 0.0_f32;
        for (idx, p) in &candidates {
            acc += p;
            if acc >= r {
                return Ok(*idx as u32);
            }
        }

        // Fallback to top candidate
        Ok(candidates[0].0 as u32)
    }
}
