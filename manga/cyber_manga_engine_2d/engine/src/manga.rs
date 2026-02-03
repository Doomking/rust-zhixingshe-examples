use crate::sd::StableDiffusion;
use ab_glyph::{FontVec, PxScale};
use anyhow::{Error, Result};
use image::{DynamicImage, GenericImage, Rgba};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Script {
    pub panels: Vec<Panel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Panel {
    pub role: String,
    pub dialogue: String,
    pub prompt: String, // Constructed from role + context
}

impl Script {
    pub fn parse(text: &str) -> Self {
        let mut panels = Vec::new();
        for line in text.lines() {
            if let Some((role, dialogue)) = line.split_once(&[':', '：'][..]) {
                let role = role.trim().to_string();
                let dialogue = dialogue.trim().to_string();

                // Simple prompt engineering based on role
                let visual_prompt = match role.as_str() {
                    "CyberGirl" | "Girl" => "1girl, cyberpunk city background, neon lights, rain, anime style, high quality, vibrant colors",
                    "Robot" | "Bot" => "mecha robot, sci-fi futuristic city, glowing eyes, metallic texture, intricate details, anime style",
                    _ => "cyberpunk atmosphere, neon lights, movie scene, anime style"
                };

                // In reality don't put dialogue in prompt, but maybe emotion?

                panels.push(Panel {
                    role,
                    dialogue,
                    prompt: visual_prompt.to_string(),
                });
            }
        }
        Self { panels }
    }
}

pub struct MangaGenerator {
    sd: std::sync::Arc<StableDiffusion>,
    font: FontVec,
}

impl MangaGenerator {
    pub fn new(sd: std::sync::Arc<StableDiffusion>) -> Result<Self> {
        // Load font
        let font_path = Path::new("assets/fonts/NotoSansSC-Regular.otf");
        let font_data = std::fs::read(font_path)
            .map_err(|e| Error::msg(format!("Failed to load font: {}", e)))?;
        let font =
            FontVec::try_from_vec(font_data).map_err(|_| Error::msg("Error constructing font"))?;

        Ok(Self { sd, font })
    }

    pub async fn generate_manga(&self, script_text: &str) -> Result<(DynamicImage, Vec<Panel>)> {
        let script = Script::parse(script_text);
        let mut images = Vec::new();

        for panel in &script.panels {
            println!("Generating panel for {}: {}", panel.role, panel.prompt);

            // Generate base image (Synchronous SD call for now, can be async if SD supports it)
            // SD generate is blocking, so we might block the thread. In a real server use spawn_blocking.
            let mut image = self.sd.generate(&panel.prompt, 30, 7.5)?;

            // Render speech bubble
            self.render_speech_bubble(&mut image, &panel.role, &panel.dialogue);

            images.push(image);
        }

        // Stitch images vertically
        if images.is_empty() {
            return Err(Error::msg("No panels generated"));
        }

        let width = images[0].width();
        let height: u32 = images.iter().map(|img| img.height()).sum();
        let spacing = 20;
        let total_height = height + (spacing * (images.len() as u32 - 1).max(0));

        let mut manga_page = DynamicImage::new_rgb8(width, total_height);
        // Fill white background
        // (Default is likely black/transparent, let's fill white if needed, but we paste over)

        let mut current_y = 0;
        for img in images {
            manga_page.copy_from(&img, 0, current_y)?;
            current_y += img.height() + spacing;
        }

        Ok((manga_page, script.panels))
    }

    fn render_speech_bubble(&self, image: &mut DynamicImage, role: &str, text: &str) {
        // Simple bottom-center bubble
        let img_width = image.width() as i32;
        let img_height = image.height() as i32;

        // Scale is strictly height in pixels for PxScale
        let scale = PxScale::from(24.0);

        // Calculate text width approx (naive, ab_glyph has layout but complex)
        // We use estimation: 24px * char_count generally works for monospace CJK, but simplified logic here.
        let text_width = (text.chars().count() as f32 * 24.0) as i32;
        let bubble_width = text_width + 40;
        let bubble_height = 60; // Fixed height for one line

        let x = (img_width - bubble_width) / 2;
        let y = img_height - bubble_height - 30;

        // Draw bubble background (White)
        draw_filled_rect_mut(
            image,
            Rect::at(x, y).of_size(bubble_width as u32, bubble_height as u32),
            Rgba([255u8, 255u8, 255u8, 255u8]),
        );

        // Draw text (Black)
        // Note: imageproc draws on RgbaImage usually. DynamicImage generic draw might be slower or need conversion.
        // We assume image is Rgb8 usually. imageproc handles GenericImage?
        // draw_text_mut takes &mut I.

        draw_text_mut(
            image,
            Rgba([0u8, 0u8, 0u8, 255u8]),
            x + 20,
            y + 15,
            scale,
            &self.font,
            text,
        );

        // Draw Role label (Optional, small on top)
        let role_scale = PxScale::from(16.0);
        draw_text_mut(
            image,
            Rgba([255u8, 0u8, 0u8, 255u8]), // Red role
            x,
            y - 20,
            role_scale,
            &self.font,
            role,
        );
    }
}
