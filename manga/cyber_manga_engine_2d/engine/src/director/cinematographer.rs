use crate::director::Storyboard;
use crate::sd::StableDiffusion;
use anyhow::Result;
use std::sync::Arc;

pub struct Cinematographer {
    sd: Arc<StableDiffusion>,
}

impl Cinematographer {
    pub fn new(sd: Arc<StableDiffusion>) -> Self {
        Self { sd }
    }

    pub fn shoot(&self, storyboard: &mut Storyboard) -> Result<()> {
        let output_dir = std::path::Path::new("output/images");
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        for shot in &mut storyboard.shots {
            // 1. Construct Prompt
            let char_tags = if let Some(c) = storyboard.characters.get(&shot.character) {
                &c.visual_tags
            } else {
                "generic character"
            };

            let action = "talking, close up, looking at viewer";
            let prompt = format!(
                "{}, {}, Studio Ghibli style, bright colors, masterpiece",
                char_tags, action
            );

            shot.visual_prompt = prompt.clone();
            println!("Shooting shot {}: {}", shot.id, prompt);

            // 2. Generate Image
            let image = self.sd.generate(&prompt, 30, 7.5)?;

            // 3. Save Image
            let filename = format!("shot_{}.png", shot.id);
            let path = output_dir.join(filename);
            image.save(&path)?;

            shot.image_path = Some(path.canonicalize().unwrap_or(path));
        }
        Ok(())
    }
}
