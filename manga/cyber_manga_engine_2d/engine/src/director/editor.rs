use ab_glyph::{FontVec, PxScale};
use anyhow::{Error, Result};
use image::{DynamicImage, Rgba};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;
use std::path::Path;

pub struct Editor {
    font: FontVec,
}

impl Editor {
    pub fn new() -> Result<Self> {
        let font_path = Path::new("assets/fonts/NotoSansSC-Regular.otf");
        let font_data = std::fs::read(font_path)
            .map_err(|e| Error::msg(format!("Failed to load font: {}", e)))?;
        let font =
            FontVec::try_from_vec(font_data).map_err(|_| Error::msg("Error constructing font"))?;

        Ok(Self { font })
    }

    pub fn edit(&self, storyboard: &mut crate::director::Storyboard) -> Result<std::path::PathBuf> {
        let output_dir = std::path::Path::new("output/video");
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        let mut clip_paths = Vec::new();

        let total_shots = storyboard.shots.len();

        for shot in &mut storyboard.shots {
            if let (Some(img_path), Some(audio_path)) = (&shot.image_path, &shot.audio_path) {
                // 1. Load Image
                let mut image = image::open(img_path)?;

                // 2. Overlay Text
                self.render_speech_bubble(
                    &mut image,
                    &shot.character,
                    &shot.dialogue,
                    shot.id,
                    total_shots,
                );

                // 3. Save Edited Image
                let edited_img_path = output_dir.join(format!("edited_{}.png", shot.id));
                image.save(&edited_img_path)?;

                // 4. Generate Clip
                let clip_path = output_dir.join(format!("clip_{}.mp4", shot.id));
                self.generate_clip(&edited_img_path, audio_path, &clip_path, shot.duration)?;

                shot.video_path = Some(clip_path.canonicalize().unwrap_or(clip_path.clone()));
                clip_paths.push(format!(
                    "file '{}'",
                    shot.video_path.as_ref().unwrap().to_str().unwrap()
                ));
            }
        }

        // 5. Concat Clips
        let list_path = output_dir.join("list.txt");
        std::fs::write(&list_path, clip_paths.join("\n"))?;

        let output_path = output_dir.join("final_movie.mp4");

        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(&list_path)
            .arg("-c")
            .arg("copy")
            .arg(&output_path)
            .status()?;

        if !status.success() {
            return Err(Error::msg("ffmpeg concat failed"));
        }

        Ok(output_path)
    }

    fn generate_clip(
        &self,
        img_path: &Path,
        audio_path: &Path,
        output_path: &Path,
        audio_duration: f64,
    ) -> Result<()> {
        // Video Duration = Audio Duration + 0.5s padding
        let video_duration = audio_duration + 0.5;

        // Ken Burns Effect: Zoom in slightly (zoom=1.0 to 1.1) over duration
        // fps=30

        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-loop").arg("1")
            .arg("-i").arg(img_path)
            .arg("-i").arg(audio_path)
            .arg("-vf").arg(format!("zoompan=z='min(zoom+0.0002,1.05)':d={}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s=896x1152:fps=30", (video_duration * 30.0) as i32)) 
            .arg("-r").arg("30")  // Explicit 30fps output
            .arg("-c:v").arg("libx264")
            .arg("-pix_fmt").arg("yuv420p")
            .arg("-c:a").arg("aac")
            .arg("-t").arg(format!("{}", video_duration))
            .arg("-shortest") 
            .arg(output_path)
            .status()?;

        if !status.success() {
            return Err(Error::msg("ffmpeg clip generation failed"));
        }
        Ok(())
    }

    pub fn render_speech_bubble(
        &self,
        image: &mut DynamicImage,
        role: &str,
        text: &str,
        bubble_idx: usize,
        total_bubbles: usize,
    ) {
        let img_width = image.width() as i32;
        let img_height = image.height() as i32;
        let scale = PxScale::from(24.0);
        let padding_x = 20;
        let padding_y = 15;
        let max_text_width = (img_width - 80).max(100); // Max width allowed for text

        // 1. Text Wrapping Logic
        let mut lines = Vec::new();
        let mut current_line = String::new();
        let char_width = 24.0; // Approximation for CJK/Monospace

        for char in text.chars() {
            let current_width = (current_line.chars().count() as f32 * char_width) as i32;
            if current_width + (char_width as i32) > max_text_width {
                lines.push(current_line);
                current_line = String::new();
            }
            current_line.push(char);
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // 2. Calculate Dimensions
        let line_height = 30;
        let text_block_height = lines.len() as i32 * line_height;
        let longest_line_width = lines
            .iter()
            .map(|l| (l.chars().count() as f32 * char_width) as i32)
            .max()
            .unwrap_or(0);

        let bubble_width = longest_line_width + (padding_x * 2);
        let bubble_height = text_block_height + (padding_y * 2);

        // 3. Position based on bubble index (simplified for now to just alternate top/bottom)
        // If bubble_idx is even -> Top, if odd -> Bottom (or vice versa? let's stick to simple logic)
        let (x, y) = if bubble_idx % 2 == 0 {
            // Top
            ((img_width - bubble_width) / 2, 50)
        } else {
            // Bottom
            (
                (img_width - bubble_width) / 2,
                img_height - bubble_height - 50,
            )
        };

        // 4. Draw Bubble (White Background with slight transparency)
        draw_filled_rect_mut(
            image,
            Rect::at(x, y).of_size(bubble_width as u32, bubble_height as u32),
            Rgba([255u8, 255u8, 255u8, 240u8]),
        );

        // 5. Draw Text Lines
        for (i, line) in lines.iter().enumerate() {
            draw_text_mut(
                image,
                Rgba([0u8, 0u8, 0u8, 255u8]),
                x + padding_x,
                y + padding_y + (i as i32 * line_height),
                scale,
                &self.font,
                line,
            );
        }

        // 6. Draw Role Label
        let role_scale = PxScale::from(16.0);
        let role_width = (role.chars().count() as f32 * 16.0) as i32;
        // Draw Role Badge Background
        draw_filled_rect_mut(
            image,
            Rect::at(x, y - 24).of_size((role_width + 10) as u32, 24),
            Rgba([0u8, 0u8, 0u8, 180u8]), // Semi-transparent black
        );
        draw_text_mut(
            image,
            Rgba([255u8, 255u8, 255u8, 255u8]), // White text
            x + 5,
            y - 22,
            role_scale,
            &self.font,
            role,
        );
    }
}
