use crate::director::{Character, Shot, Storyboard};
use crate::script_ai::ScriptAI;
use anyhow::Result;
use std::collections::HashMap;

pub struct Screenwriter {
    use_ai: bool,
    device: candle_core::Device,
}

impl Screenwriter {
    pub fn new(device: &candle_core::Device, use_ai: bool) -> Result<Self> {
        Ok(Self {
            use_ai,
            device: device.clone(),
        })
    }

    pub fn write(&self, script_text: &str) -> Result<Storyboard> {
        // If AI parser is enabled, instantiate it on demand, use it, then drop it
        if self.use_ai {
            println!("🤖 Initializing AI Script Parser (On-Demand)...");
            // This scope ensures ScriptAI is dropped after use
            let ai = ScriptAI::new(&self.device)?;
            println!("🤖 Using AI mode to parse casual text...");
            let panels = ai.parse_casual_text(script_text)?;
            return self.panels_to_storyboard(panels, Some(&ai));
            // We pass &ai if we need translation features inside panels_to_storyboard
        }

        // Otherwise, parse as structured markdown
        println!("📄 Using markdown parser...");
        self.parse_markdown(script_text)
    }

    /// Parse structured markdown script (Panel-based format)
    fn parse_markdown(&self, script_text: &str) -> Result<Storyboard> {
        let mut shots = Vec::new();
        let mut characters = HashMap::new();
        let mut shot_id = 0;

        // Parse script by panels
        let mut current_panel_name = String::new();
        let mut current_scene_desc = String::new();
        let mut current_dialogue_lines = Vec::new();
        let mut current_character = String::new();

        for line in script_text.lines() {
            let line = line.trim();

            // Panel marker: #### **Panel X:
            if line.starts_with("####") && line.contains("Panel") {
                println!("🔍 Found panel marker: {}", line);

                // Save previous panel if exists
                if !current_panel_name.is_empty() {
                    println!("💾 Saving previous panel: {}", current_panel_name);
                    self.create_shot(
                        &mut shots,
                        &mut characters,
                        &mut shot_id,
                        &current_panel_name,
                        &current_scene_desc,
                        &current_dialogue_lines,
                        &current_character,
                        None,
                    );
                }

                // Start new panel
                current_panel_name = line
                    .trim_start_matches("####")
                    .trim_start_matches("**")
                    .trim_end_matches("**")
                    .trim()
                    .to_string();
                current_scene_desc.clear();
                current_dialogue_lines.clear();
                current_character.clear();
                continue;
            }

            // Scene description section (after 画面描述)
            if line.contains("画面描述") {
                // Read next lines until we hit dialogue or other section
                current_scene_desc.clear();
                continue;
            }

            // Accumulate scene description lines
            if !current_scene_desc.is_empty()
                || (line.starts_with("*") && !line.contains("对白") && !line.contains("音效"))
            {
                if !line.contains("对白")
                    && !line.contains("音效")
                    && !line.contains("心理活动")
                    && !line.is_empty()
                {
                    let desc_line = line.trim_start_matches("*").trim();
                    if !desc_line.is_empty() && !desc_line.starts_with("**") {
                        current_scene_desc.push_str(desc_line);
                        current_scene_desc.push(' ');
                    }
                }
                continue;
            }

            // Dialogue section (对白 or 心理活动)
            if line.contains("对白") || line.contains("心理活动") || line.contains("独白") {
                continue; // Skip header
            }

            // SFX section - skip
            if line.contains("音效") {
                continue;
            }

            // Extract actual dialogue: Mio："..."
            if (line.contains("：") || line.contains(":")) && line.contains("\"") {
                if let Some(colon_pos) = line.find(&['：', ':'][..]) {
                    let character_part = line[..colon_pos].trim_start_matches("*").trim();
                    let dialogue_part = line[colon_pos + 1..].trim();

                    if !dialogue_part.is_empty() {
                        current_character = character_part.to_string();
                        current_dialogue_lines.push(dialogue_part.to_string());
                    }
                }
            }
        }

        // Don't forget the last panel
        if !current_panel_name.is_empty() {
            println!("💾 Saving last panel: {}", current_panel_name);
            self.create_shot(
                &mut shots,
                &mut characters,
                &mut shot_id,
                &current_panel_name,
                &current_scene_desc,
                &current_dialogue_lines,
                &current_character,
                None,
            );
        }

        println!("📊 Total shots created: {}", shots.len());

        Ok(Storyboard {
            title: "AI Manga".to_string(),
            shots,
            characters,
        })
    }

    /// Convert AI-parsed panels to storyboard
    fn panels_to_storyboard(
        &self,
        panels: Vec<crate::script_ai::Panel>,
        ai: Option<&ScriptAI>,
    ) -> Result<Storyboard> {
        let mut shots = Vec::new();
        let mut characters = HashMap::new();

        for (idx, panel) in panels.iter().enumerate() {
            println!("🎬 Converting Panel {} to shot", idx + 1);

            // Combine all dialogues for this panel
            let combined_dialogue = panel
                .dialogues
                .iter()
                .map(|d| d.text.clone())
                .collect::<Vec<_>>()
                .join(" ");

            // Get speaker (use first dialogue's speaker, or "Narrator")
            let speaker = panel
                .dialogues
                .first()
                .map(|d| d.speaker.clone())
                .unwrap_or_else(|| "Narrator".to_string());

            // Register character if not exists
            // Register character if not exists
            if !characters.contains_key(&speaker) && speaker != "Narrator" {
                // Sanitize speaker name for visual tags (remove non-ascii)
                // If speaker is Chinese, use "1girl" or "1boy" based on simple heuristic or default
                let visual_name = if speaker.chars().all(|c| c.is_ascii()) {
                    speaker.to_lowercase()
                } else {
                    "girl".to_string() // Default to girl for anime style if name is Chinese
                };

                characters.insert(
                    speaker.clone(),
                    Character {
                        name: speaker.clone(),
                        voice_id: "female".to_string(), // Default voice
                        visual_tags: format!(
                            "1{}, {}, anime style, high quality",
                            visual_name, visual_name
                        ),
                    },
                );
            }

            // Create shot

            // Use sd_prompt directly if LLM provided English SD tags
            let visual_prompt = if let Some(ref sd_prompt) = panel.sd_prompt {
                // LLM already generated English SD prompt - use directly!
                println!("✅ Using LLM-generated SD prompt: {}", sd_prompt);
                sd_prompt.clone()
            } else {
                // Fallback: translate Chinese descriptions to English
                println!("🔄 No sd_prompt, falling back to translation...");

                let background_prompt = if let Some(parser) = ai {
                    if panel.background_visual.chars().any(|c| !c.is_ascii()) {
                        println!("🔄 Translating scene description...");
                        match parser.translate(panel.background_visual.trim()) {
                            Ok(en) => {
                                println!("✅ Translated: {}", en);
                                en
                            }
                            Err(e) => {
                                println!("⚠️ Translation failed: {}", e);
                                panel.background_visual.clone()
                            }
                        }
                    } else {
                        panel.background_visual.clone()
                    }
                } else {
                    panel.background_visual.clone()
                };

                let character_prompt = if let Some(parser) = ai {
                    if panel.character_visual.chars().any(|c| !c.is_ascii()) {
                        println!("🔄 Translating character description...");
                        match parser.translate(panel.character_visual.trim()) {
                            Ok(en) => {
                                println!("✅ Translated character: {}", en);
                                en
                            }
                            Err(e) => {
                                println!("⚠️ Character translation failed: {}", e);
                                "1girl, anime, detailed face, expressive eyes".to_string()
                            }
                        }
                    } else {
                        panel.character_visual.clone()
                    }
                } else {
                    panel.character_visual.clone()
                };

                format!("{}, {}", background_prompt, character_prompt)
            };

            let background_prompt = if panel.sd_prompt.is_some() {
                // When using sd_prompt, background_prompt is just for reference
                panel.background_visual.clone()
            } else {
                visual_prompt
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };

            let shot = Shot {
                id: idx,
                panel_name: format!("Panel {}", idx + 1),
                action: String::new(),
                character: speaker,
                dialogue: combined_dialogue,
                scene_description: panel.background_visual.clone(),
                background_prompt,
                visual_prompt,
                audio_path: None,
                image_paths: Vec::new(),
                video_path: None,
                duration: 0.0,
            };

            shots.push(shot);
        }

        println!(
            "✅ Converted {} panels to {} shots",
            panels.len(),
            shots.len()
        );

        Ok(Storyboard {
            title: "AI Manga".to_string(),
            shots,
            characters,
        })
    }

    fn create_shot(
        &self,
        shots: &mut Vec<Shot>,
        characters: &mut HashMap<String, Character>,
        shot_id: &mut usize,
        panel_name: &str,
        scene_desc: &str,
        dialogue_lines: &[String],
        character_name: &str,
        ai: Option<&ScriptAI>,
    ) {
        if dialogue_lines.is_empty() && scene_desc.is_empty() {
            return; // Skip empty panels
        }

        // Combine all dialogue
        let combined_dialogue = dialogue_lines.join(" ");

        // Auto-discover character if new
        if !character_name.is_empty() && !characters.contains_key(character_name) {
            let available_voices = vec![
                "zh-CN-XiaoxiaoNeural", // Female
                "zh-CN-YunxiNeural",    // Male
                "zh-CN-XiaoyiNeural",   // Female
                "zh-CN-YunjianNeural",  // Male
            ];
            let voice_idx = characters.len() % available_voices.len();
            let selected_voice = available_voices[voice_idx];

            let character = Character {
                name: character_name.to_string(),
                voice_id: selected_voice.to_string(),
                visual_tags: format!("1girl, {}, Studio Ghibli style", character_name),
            };
            characters.insert(character_name.to_string(), character);
        }

        // Translate scene description if AI is available
        let background_prompt = if let Some(parser) = ai {
            // Only translate if it looks like Chinese (simple heuristic: contains non-ascii)
            if scene_desc.chars().any(|c| !c.is_ascii()) {
                println!("🔄 Translating scene description...");
                match parser.translate(scene_desc.trim()) {
                    Ok(en) => {
                        println!("✅ Translated: {}", en);
                        en
                    }
                    Err(e) => {
                        println!("⚠️ Translation failed: {}", e);
                        scene_desc.trim().to_string()
                    }
                }
            } else {
                scene_desc.trim().to_string()
            }
        } else {
            scene_desc.trim().to_string()
        };

        let shot = Shot {
            id: *shot_id,
            panel_name: panel_name.to_string(),
            character: character_name.to_string(),
            action: String::new(), // Initialized as empty
            dialogue: combined_dialogue,
            scene_description: scene_desc.trim().to_string(),
            background_prompt: background_prompt, // Use translated prompt
            visual_prompt: String::new(),         // Will be filled by Cinematographer
            audio_path: None,
            image_paths: Vec::new(),
            video_path: None,
            duration: 4.0, // Longer default for panels
        };

        shots.push(shot);
        *shot_id += 1;
    }
}
