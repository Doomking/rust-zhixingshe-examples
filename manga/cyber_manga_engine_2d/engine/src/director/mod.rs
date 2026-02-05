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
    pub panel_name: String, // e.g., "Panel 1: 宁静的开场"
    pub character: String,  // Who is speaking/acting?
    pub dialogue: String,   // Only actual dialogue (speech)

    // Director's instructions
    pub scene_description: String, // From script (画面描述)
    pub background_prompt: String, // Used for seeding (Scene consistency)
    pub visual_prompt: String,     // Full prompt for SD (combined)
    pub audio_path: Option<PathBuf>,
    pub image_paths: Vec<PathBuf>,
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
    pub fn new(
        sd: std::sync::Arc<crate::sd::StableDiffusion>,
        use_ai_parser: bool,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            screenwriter: screenwriter::Screenwriter::new(use_ai_parser)?,
            cinematographer: cinematographer::Cinematographer::new(sd),
            sound_engineer: audio::SoundEngineer::new(),
            editor: editor::Editor::new()?,
        })
    }

    pub async fn produce(&self, script_text: &str) -> anyhow::Result<(Storyboard, PathBuf)> {
        // 1. Script -> Storyboard
        let mut storyboard = self.screenwriter.write(script_text)?;
        println!("Storyboard created with {} shots", storyboard.shots.len());

        // 2. Audio (Shot) - NOW FIRST to determine duration
        self.sound_engineer
            .construct_soundtrack(&mut storyboard)
            .await?;
        println!("Soundtrack recorded");

        // 3. Storyboard -> Images (Shot) - NOW SECOND to use duration
        self.cinematographer.shoot(&mut storyboard)?;
        println!("Cinematography complete");

        // 4. Editor -> Video
        let final_video = self.editor.edit(&mut storyboard)?;
        println!("Editing complete. Output: {:?}", final_video);

        Ok((storyboard, final_video))
    }
}
