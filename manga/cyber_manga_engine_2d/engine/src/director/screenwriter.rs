use crate::director::{Character, Shot, Storyboard};
use anyhow::Result;
use std::collections::HashMap;

pub struct Screenwriter {}

impl Screenwriter {
    pub fn new() -> Self {
        Self {}
    }

    pub fn write(&self, script_text: &str) -> Result<Storyboard> {
        let mut shots = Vec::new();
        let mut characters = HashMap::new();

        // Basic parsing logic (to be enhanced)
        for (idx, line) in script_text.lines().enumerate() {
            if let Some((role, dialogue)) = line.split_once(&[':', '：'][..]) {
                let role = role.trim().to_string();
                let dialogue = dialogue.trim().to_string();

                // Auto-discover character if new
                if !characters.contains_key(&role) {
                    // Voice Diversity Logic:
                    // Assign a voice based on the order of appearance (characters.len()).
                    let available_voices = vec![
                        "zh-CN-XiaoxiaoNeural", // Female
                        "zh-CN-YunxiNeural",    // Male
                        "zh-CN-XiaoyiNeural",   // Female
                        "zh-CN-YunjianNeural",  // Male
                        "zh-CN-XiaouNeural",    // Female
                        "zh-CN-LibaoNeural",    // Male (Child?)
                    ];
                    let voice_idx = characters.len() % available_voices.len();
                    let selected_voice = available_voices[voice_idx];

                    let character = Character {
                        name: role.clone(),
                        voice_id: selected_voice.to_string(),
                        visual_tags: format!(
                            "{}, matches role {}, Studio Ghibli style",
                            role, role
                        ),
                    };
                    characters.insert(role.clone(), character);
                }

                // Create Shot
                let shot = Shot {
                    id: idx,
                    character: role.clone(),
                    dialogue,
                    visual_prompt: String::new(), // Will be filled by Cinematographer
                    audio_path: None,
                    image_paths: Vec::new(),
                    video_path: None,
                    duration: 3.0,
                };
                shots.push(shot);
            }
        }

        Ok(Storyboard {
            title: "AI Manga".to_string(),
            shots,
            characters,
        })
    }
}
