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
            println!(
                "🎬 Shooting Panel: {} (Character: {})",
                shot.panel_name, shot.character
            );

            shot.image_paths = Vec::new();

            // Determine visual prompt based on scene_description
            // Enhanced style prompt for "Fresh & Airy" anime look
            let style_prefix = "Studio Ghibli style, Hayao Miyazaki, vivid colors, fresh atmosphere, bright and airy, clean lines, cel shaded, anime screencap, masterpiece, best quality, ultra detailed, 8k resolution, cinema composition, wide angle, Makoto Shinkai clouds, nature focus";

            // Fix: Use shot.visual_prompt which contains both Background + Character Visuals from ScriptAI
            let full_prompt = if !shot.background_prompt.is_empty() {
                // Even if we have background prompt, we MUST include valid character description.
                // shot.visual_prompt has "Background..., Character..."
                format!("{}, {}", style_prefix, shot.visual_prompt)
            } else {
                // Fallback
                format!("{}, {}", style_prefix, shot.visual_prompt)
            };

            let neg_prompt = "lowres, bad anatomy, bad hands, text, error, missing fingers, extra digit, fewer digits, cropped, worst quality, low quality, normal quality, jpeg artifacts, signature, watermark, username, blurry, artist name, chinese text, monochrome, grayscale";

            println!("  🎨 Shooting shot {}: {}", shot.id, full_prompt);

            // 2. Generate Images with SEED LOCKING
            // Hash the background_prompt to get a stable seed for this scene
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            shot.background_prompt.hash(&mut hasher);
            let seed = hasher.finish();

            // Generate 2-3 images per panel
            let num_frames = ((shot.duration / 3.0).ceil() as usize).max(2).min(4);

            for i in 0..num_frames {
                println!(
                    "  📸 Generating image {}/{} (Seed: {})",
                    i + 1,
                    num_frames,
                    seed
                );

                // We want slightly different noise for Animation flow?
                // If we use EXACT same seed, we get EXACT same image if prompt matches.
                // But prompt has Character description which might change slightly if we had finer control.
                // Here prompt is static for the shot. So we get static image.
                // BUT we want "Character Moving".
                // If we change seed slightly? `seed + i as u64`?
                // Then background changes completely.

                // Goal: Static Background, Moving Character.
                // Hard with T2I.
                // Best compromise: Use SAME seed for all frames in the scene?
                // If prompt is identical, image is identical.
                // We are generating multiple frames for ZOOM/PAN effect in `editor.rs` (ken burns).
                // `editor.rs` takes ONE video path?
                // Wait, `Cinematographer` generates multiple images... wait, `shot.image_paths` is Vec.
                // Does `editor.rs` use multiple images?
                // `editor.rs`: `let shot_visual = ...`
                // `editor.rs` loop logic:
                // It uses `shot.image_paths[0]` usually?
                // Let's check `editor.rs`.
                // If `editor.rs` only uses one image, then generating multiple is waste.

                // Let's assume we want ONE good image per shot for now to fix consistency.
                // And for Scene Consistency across SHOTS (Panel 1 vs Panel 2):
                // Panel 1: "Background A, Char Action A" -> Seed(Background A)
                // Panel 2: "Background A, Char Action B" -> Seed(Background A)
                // Result: SD generates similar layout because seed is same, but pixels will change because prompt changed.
                // This is the best we can do without ControlNet.

                let image = self.sd.generate(&full_prompt, 30, 7.5, Some(seed))?;

                let filename = format!("shot_{}_frame_{}.png", shot.id, i);
                let path = output_dir.join(filename);
                image.save(&path)?;

                shot.image_paths.push(path.canonicalize().unwrap_or(path));
            }
        }
        Ok(())
    }
}
