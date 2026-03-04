use crate::models::sd15::SD15;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub mod audio;
pub mod cinematographer;
pub mod editor;
pub mod screenwriter;

#[derive(Debug, Clone, Serialize)]
pub struct Character {
    pub name: String,
    pub voice_id: String,
    pub visual_tags: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Shot {
    pub id: usize,
    pub panel_name: String,
    pub action: String,
    pub dialogue: String,
    pub character: String,
    pub visual_prompt: String,
    pub background_prompt: String,
    pub scene_description: String, // Added back
    pub video_path: Option<PathBuf>,
    pub image_paths: Vec<PathBuf>,
    pub audio_path: Option<String>,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Storyboard {
    pub title: String,                                            // Added back
    pub characters: std::collections::HashMap<String, Character>, // Added back
    pub shots: Vec<Shot>,
}

pub struct Director {
    device: candle_core::Device,
    screenwriter: screenwriter::Screenwriter,
    sound_engineer: audio::SoundEngineer,
    editor: editor::Editor,
    sd_model: Arc<Mutex<SD15>>,
}

impl Director {
    pub fn new(device: &candle_core::Device, use_ai_parser: bool) -> anyhow::Result<Self> {
        // Initialize SD15 (Ghibli)
        println!("🚀 Initializing Ghibli-Diffusion Engine (Candle)...");
        let sd15 = SD15::new(device)?;
        // Wrap in Mutex for mutable access in Cinematographer (text_to_image needs &mut self)
        let sd_model = Arc::new(Mutex::new(sd15));

        Ok(Self {
            device: device.clone(),
            screenwriter: screenwriter::Screenwriter::new(device, use_ai_parser)?,
            sound_engineer: audio::SoundEngineer::new(),
            editor: editor::Editor::new()?,
            sd_model,
        })
    }

    pub async fn produce(
        &self,
        script_text: &str,
        style: Option<String>,
    ) -> anyhow::Result<(Storyboard, PathBuf)> {
        // 1. Script -> Storyboard
        let mut storyboard = self.screenwriter.write(script_text)?;
        println!("Storyboard created with {} shots", storyboard.shots.len());

        // 2. Audio - Generate TTS for dialogues
        // Assuming sound_engineer has appropriate method, checking previous file content suggests construct_soundtrack or record_dialogues
        // The view_file showed `construct_soundtrack`.
        self.sound_engineer
            .construct_soundtrack(&mut storyboard)
            .await?;
        println!("Soundtrack recorded");

        // 3. Generate Manga Panels (Images)
        let style_str = style.as_deref().unwrap_or("ghibli");
        println!("🎨 Generating Manga Panels (Style: {})...", style_str);

        // Cinematographer now takes Arc<Mutex<SD15>> via new()
        let cinematographer = cinematographer::Cinematographer::new(self.sd_model.clone());
        cinematographer.shoot(&mut storyboard, style_str).await?;
        println!("📸 Photography complete");

        // 4. Editor: Layout -> Manga Page
        let final_page = self.editor.edit(&mut storyboard)?;
        println!("✅ Manga Page ready: {:?}", final_page);

        Ok((storyboard, final_page))
    }
}
