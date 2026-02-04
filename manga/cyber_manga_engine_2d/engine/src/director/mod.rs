use serde::Serialize;
use std::path::PathBuf;

pub mod audio;
pub mod cinematographer;
pub mod editor;
pub mod screenwriter;

#[derive(Debug, Clone, Serialize)]
pub struct Character {
    pub name: String,
    pub voice_id: String,
    pub visual_tags: String, // e.g., "1girl, blue hair"
}

#[derive(Debug, Clone, Serialize)]
pub struct Shot {
    pub id: usize,
    pub character: String, // Who is speaking/acting?
    pub dialogue: String,

    // Director's instructions
    pub visual_prompt: String, // Full prompt for SD
    pub audio_path: Option<PathBuf>,
    pub image_path: Option<PathBuf>,
    pub video_path: Option<PathBuf>, // Path to the generated clip (image + audio + zoompan)
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Storyboard {
    pub title: String,
    pub shots: Vec<Shot>,
    pub characters: std::collections::HashMap<String, Character>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub location: String,
    pub shots: Vec<Shot>,
}

pub struct Director {
    // Orchestrator
    screenwriter: screenwriter::Screenwriter,
    cinematographer: cinematographer::Cinematographer,
    sound_engineer: audio::SoundEngineer,
    editor: editor::Editor,
}

impl Director {
    pub fn new(sd: std::sync::Arc<crate::sd::StableDiffusion>) -> anyhow::Result<Self> {
        Ok(Self {
            screenwriter: screenwriter::Screenwriter::new(),
            cinematographer: cinematographer::Cinematographer::new(sd),
            sound_engineer: audio::SoundEngineer::new(),
            editor: editor::Editor::new()?,
        })
    }

    pub fn produce(&self, script_text: &str) -> anyhow::Result<(Storyboard, PathBuf)> {
        // 1. Script -> Storyboard
        let mut storyboard = self.screenwriter.write(script_text)?;
        println!("Storyboard created with {} shots", storyboard.shots.len());

        // 2. Storyboard -> Images (Shot)
        self.cinematographer.shoot(&mut storyboard)?;
        println!("Cinematography complete");

        // 3. Audio (Shot)
        self.sound_engineer.construct_soundtrack(&mut storyboard)?;
        println!("Soundtrack recorded");

        // 4. Editor -> Video
        let final_video = self.editor.edit(&mut storyboard)?;
        println!("Editing complete. Output: {:?}", final_video);

        Ok((storyboard, final_video))
    }
}
