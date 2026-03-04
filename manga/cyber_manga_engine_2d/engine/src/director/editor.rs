use ab_glyph::{FontRef, FontVec, PxScale};
use anyhow::{Error, Result};
use image::{DynamicImage, GenericImage, GenericImageView, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_ellipse_mut, draw_text_mut, text_size};
use std::path::Path;

pub struct Editor {
    font: FontVec,
}

impl Editor {
    pub fn new() -> Result<Self> {
        let font_path = Path::new("assets/fonts/NotoSansSC-Regular.otf");
        if !font_path.exists() {
            // If font missing, try system font or return error
            return Err(Error::msg(format!("Font not found at {:?}", font_path)));
        }
        let font_data = std::fs::read(font_path)
            .map_err(|e| Error::msg(format!("Failed to load font: {}", e)))?;
        let font =
            FontVec::try_from_vec(font_data).map_err(|_| Error::msg("Error constructing font"))?;

        Ok(Self { font })
    }

    /// Assemble shots into a Manga Page
    pub fn edit(&self, storyboard: &mut crate::director::Storyboard) -> Result<std::path::PathBuf> {
        let output_dir = std::path::Path::new("output/manga");
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        // Collect all panel images
        let mut panel_images = Vec::new();
        for shot in &storyboard.shots {
            if let Some(path) = shot.image_paths.first() {
                let img = image::open(path)
                    .map_err(|e| Error::msg(format!("Failed to open image {:?}: {}", path, e)))?;
                panel_images.push((img, shot.dialogue.clone()));
            }
        }

        if panel_images.is_empty() {
            return Err(Error::msg("No images found in storyboard to edit"));
        }

        // Layout Configuration
        let panel_width = 512;
        let panel_height = 768; // Portrait panels
        let gutter = 20;
        let margin = 40;
        let cols = 2;
        let rows = (panel_images.len() + cols - 1) / cols;

        let page_width = margin * 2 + (panel_width * cols as u32) + (gutter * (cols as u32 - 1));
        let page_height = margin * 2 + (panel_height * rows as u32) + (gutter * (rows as u32 - 1));

        let mut canvas = RgbaImage::from_pixel(page_width, page_height, Rgba([255, 255, 255, 255]));

        for (i, (img, dialogue)) in panel_images.iter().enumerate() {
            let row = (i / cols) as u32;
            let col = (i % cols) as u32;

            let x = margin + col * (panel_width + gutter);
            let y = margin + row * (panel_height + gutter);

            // Resize image to fit panel slot exactly
            let resized = img.resize_to_fill(
                panel_width,
                panel_height,
                image::imageops::FilterType::Lanczos3,
            );

            // Copy to canvas
            image::imageops::overlay(&mut canvas, &resized, x as i64, y as i64);

            // Draw Speech Bubble if dialogue exists
            if !dialogue.is_empty() {
                self.draw_speech_bubble(&mut canvas, x, y, panel_width, panel_height, dialogue);
            }
        }

        let output_path = output_dir.join("manga_page.png");
        canvas.save(&output_path)?;

        Ok(output_path)
    }

    fn draw_speech_bubble(
        &self,
        canvas: &mut RgbaImage,
        panel_x: u32,
        panel_y: u32,
        panel_w: u32,
        panel_h: u32,
        text: &str,
    ) {
        let scale = PxScale { x: 24.0, y: 24.0 };
        let text_color = Rgba([0, 0, 0, 255]);
        let bubble_color = Rgba([255, 255, 255, 230]); // Semi-transparent white

        let (w, h) = text_size(scale, &self.font, text);
        let bubble_w = w as u32 + 40;
        let bubble_h = h as u32 + 30;

        let bubble_cx = panel_x + panel_w / 2;
        let bubble_cy = panel_y + panel_h - bubble_h / 2 - 40; // 40px from bottom

        // Draw Bubble (Ellipse)
        // Note: imageproc ellipse takes (center_x, center_y, radius_x, radius_y)
        draw_filled_ellipse_mut(
            canvas,
            (bubble_cx as i32, bubble_cy as i32),
            (bubble_w / 2) as i32,
            (bubble_h / 2) as i32,
            bubble_color,
        );

        // Draw Text
        // Center text in bubble
        let text_x = bubble_cx - (w as u32 / 2);
        let text_y = bubble_cy - (h as u32 / 2);

        draw_text_mut(
            canvas,
            text_color,
            text_x as i32,
            text_y as i32,
            scale,
            &self.font,
            text,
        );
    }
}
