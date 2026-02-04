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

        for shot in &mut storyboard.shots {
            // Check if audio exists and we have at least one image
            if let (Some(audio_path), Some(img_path)) = (&shot.audio_path, shot.image_paths.first())
            {
                // 1. Process Keyframe (Resize + Subtitles)
                let image = image::open(img_path)?;

                // Resize to 720x1280 (Lanczos3)
                let target_w = 720;
                let target_h = 1280;
                let mut bg_image =
                    image.resize_to_fill(target_w, target_h, image::imageops::FilterType::Lanczos3);

                // Overlay Subtitles
                // NEW STYLE: No Box, No Name, Outlined Text, Wider
                self.render_subtitle(&mut bg_image, &shot.dialogue);

                let processed_path = output_dir.join(format!("processed_{}.png", shot.id));
                bg_image.save(&processed_path)?;

                // 2. Generate Video Clip for this Shot
                // Visual only clip with Smooth Zoom (Ken Burns)
                let shot_visual = output_dir.join(format!("shot_visual_{}.mp4", shot.id));
                self.generate_visual_clip(&processed_path, &shot_visual, shot.duration)?;

                // 3. Merge with Audio
                let final_shot_clip = output_dir.join(format!("clip_{}.mp4", shot.id));
                // -shortest to clip visual if audio is shorter (or vice versa, usually audio dictates)
                let status2 = std::process::Command::new("ffmpeg")
                    .arg("-y")
                    .arg("-i")
                    .arg(&shot_visual)
                    .arg("-i")
                    .arg(audio_path)
                    .arg("-c:v")
                    .arg("copy")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-shortest") // Cut to shortest stream logic
                    .arg(&final_shot_clip)
                    .status()?;

                if !status2.success() {
                    return Err(Error::msg("Failed to merge audio"));
                }

                shot.video_path = Some(
                    final_shot_clip
                        .canonicalize()
                        .unwrap_or(final_shot_clip.clone()),
                );
                clip_paths.push(format!(
                    "file '{}'",
                    shot.video_path
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
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

    fn generate_visual_clip(
        &self,
        img_path: &Path,
        output_path: &Path,
        duration: f64,
    ) -> Result<()> {
        // Zoom/Pan for mobile
        let w = 720;
        let h = 1280;

        // OPTIMIZATION: Smooth continuous zoom (Ken Burns)
        // zoom+0.001 per frame @ 30fps = ~0.03 zoom per second.
        // This eliminates jitter by using one image.
        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(img_path)
            .arg("-vf")
            .arg(format!("zoompan=z='min(zoom+0.001,1.5)':d={}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s={}x{}:fps=30", (duration * 30.0) as i32, w, h))
            .arg("-r")
            .arg("30")
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-t")
            .arg(format!("{}", duration))
            .arg(output_path)
            .status()?;

        if !status.success() {
            return Err(Error::msg("ffmpeg visual clip generation failed"));
        }
        Ok(())
    }

    pub fn render_subtitle(&self, image: &mut DynamicImage, text: &str) {
        let img_width = image.width() as i32;
        let img_height = image.height() as i32;

        // OPTIMIZATION: Even smaller font (32.0) to ensure single line fit.
        let scale = PxScale::from(32.0);
        let padding_x = 20;
        let _padding_y = 20;
        let max_text_width = (img_width - (padding_x * 2)).max(100);

        // 1. Text Wrapping Logic
        let mut lines = Vec::new();
        let mut current_line = String::new();
        let char_width = 40.0; // Approx for larger font

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

        // 2. Position: Bottom center
        let line_height = 50;
        let total_text_height = lines.len() as i32 * line_height;
        let y_start = img_height - total_text_height - 100; // 100px from bottom

        // 3. Draw Text with Outline
        for (i, line) in lines.iter().enumerate() {
            let y_pos = y_start + (i as i32 * line_height);
            let x_pos = padding_x;

            // Simulate Stroke (Outline) - Draw black text at offsets
            let offsets = [
                (-2, -2),
                (-2, 2),
                (2, -2),
                (2, 2),
                (0, -2),
                (0, 2),
                (-2, 0),
                (2, 0),
            ];
            for (ox, oy) in offsets {
                draw_text_mut(
                    image,
                    Rgba([0u8, 0u8, 0u8, 255u8]), // Black
                    x_pos + ox,
                    y_pos + oy,
                    scale,
                    &self.font,
                    line,
                );
            }

            // Draw Inner White Text
            draw_text_mut(
                image,
                Rgba([255u8, 255u8, 255u8, 255u8]), // White
                x_pos,
                y_pos,
                scale,
                &self.font,
                line,
            );
        }
    }
}
