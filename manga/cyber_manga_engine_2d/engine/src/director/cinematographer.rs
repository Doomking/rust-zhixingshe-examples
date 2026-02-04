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
            let duration = shot.duration;
            // Strategy: 1 image every 2.5 seconds, minimum 1.
            let num_frames = (duration / 2.5).ceil() as usize;
            let num_frames = if num_frames < 1 { 1 } else { num_frames };

            println!(
                "Shooting shot {}: duration {:.1}s -> {} frames",
                shot.id, duration, num_frames
            );

            shot.image_paths = Vec::new();

            // 1. Construct Prompt Base
            let char_tags = if let Some(c) = storyboard.characters.get(&shot.character) {
                &c.visual_tags
            } else {
                "1girl, solo, anime style"
            };

            // Enhanced Ghibli Prompt
            let style_prompt = "Studio Ghibli style, Hayao Miyazaki, anime screencap, cel shaded, vibrant colors, detailed background, masterpiece, best quality, 8k";
            let _neg_prompt =
                "low quality, bad anatomy, worst quality, text, watermark, signature, ugly";

            // Allow slight variation per frame if we wanted, but for now consistent prompt + different seed (automatic in generate?)
            // Actually generate() likely uses random seed each time.

            let action = "talking, looking at viewer, expressive eyes"; // Can vary this slightly per frame if desired

            let full_prompt = format!("{}, {}, {}", style_prompt, char_tags, action);
            shot.visual_prompt = full_prompt.clone();

            for i in 0..num_frames {
                println!("  - Generating frame {}/{}", i + 1, num_frames);
                // 2. Generate Image
                // We might want slightly different noise or prompt for each frame to avoid static look?
                // SD generation usually involves random seed, so images will differ naturally.
                // To make them consistent but moving, we'd need ControlNet or similar, which we don't have.
                // So "different images" is the best we can do for "movement" (jittery style).

                let image = self.sd.generate(&full_prompt, 30, 7.5)?;

                // 3. Save Image
                let filename = format!("shot_{}_frame_{}.png", shot.id, i);
                let path = output_dir.join(filename);
                image.save(&path)?;

                shot.image_paths.push(path.canonicalize().unwrap_or(path));
            }
        }
        Ok(())
    }
}
