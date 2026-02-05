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
            // ROUND 9: Ensure we have images (Audio is now optional)
            if !shot.image_paths.is_empty() {
                let num_images = shot.image_paths.len();
                let mut processed_paths = Vec::new();

                // 1. Process all images (Resize + Subtitles)
                for (idx, img_path) in shot.image_paths.iter().enumerate() {
                    let image = image::open(img_path)?;
                    let target_w = 720;
                    let target_h = 1280;
                    let mut bg_image = image.resize_to_fill(
                        target_w,
                        target_h,
                        image::imageops::FilterType::Lanczos3,
                    );

                    // Overlay Subtitles on each image
                    self.render_subtitle(&mut bg_image, &shot.dialogue);

                    let processed_path =
                        output_dir.join(format!("processed_{}_{}.png", shot.id, idx));
                    bg_image.save(&processed_path)?;
                    processed_paths.push(processed_path);
                }

                // 2. Generate video clips for each image
                let clip_duration = shot.duration / (num_images as f64);
                let mut image_clips = Vec::new();

                for (idx, img_path) in processed_paths.iter().enumerate() {
                    let clip_path = output_dir.join(format!("img_clip_{}_{}.mp4", shot.id, idx));
                    self.generate_visual_clip(img_path, &clip_path, clip_duration)?;
                    image_clips.push(clip_path);
                }

                // 3. Crossfade between clips (if more than 1)
                let shot_visual = output_dir.join(format!("shot_visual_{}.mp4", shot.id));

                if image_clips.len() == 1 {
                    // Single image, just copy
                    std::fs::copy(&image_clips[0], &shot_visual)?;
                } else {
                    // Multiple images: use xfade filter
                    self.crossfade_clips(&image_clips, &shot_visual, clip_duration)?;
                }

                // 4. Merge with Audio OR Just Copy if Silent
                let final_shot_clip = output_dir.join(format!("clip_{}.mp4", shot.id));

                if let Some(audio_path) = &shot.audio_path {
                    // Has audio: Merge
                    let status = std::process::Command::new("ffmpeg")
                        .arg("-y")
                        .arg("-i")
                        .arg(&shot_visual)
                        .arg("-i")
                        .arg(audio_path)
                        .arg("-c:v")
                        .arg("copy")
                        .arg("-c:a")
                        .arg("aac")
                        .arg("-shortest")
                        .arg(&final_shot_clip)
                        .status()?;

                    if !status.success() {
                        return Err(Error::msg("Failed to merge audio"));
                    }
                } else {
                    // Silent: Generate silent audio track so concat works
                    // ffmpeg -f lavfi -i anullsrc=cl=mono:r=24000 -i shot_visual.mp4 -c:v copy -c:a aac -shortest final.mp4
                    let status = std::process::Command::new("ffmpeg")
                        .arg("-y")
                        .arg("-f")
                        .arg("lavfi")
                        .arg("-i")
                        .arg("anullsrc=channel_layout=mono:sample_rate=24000")
                        .arg("-i")
                        .arg(&shot_visual)
                        .arg("-c:v")
                        .arg("copy")
                        .arg("-c:a")
                        .arg("aac")
                        .arg("-shortest")
                        .arg(&final_shot_clip)
                        .status()?;

                    if !status.success() {
                        // Fallback if lavfi fails: just copy (but this might break concat audio)
                        println!("⚠️ Failed to add silent audio, falling back to video only");
                        std::fs::copy(&shot_visual, &final_shot_clip)?;
                    }
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
        let w = 720;
        let h = 1280;

        // ROUND 5: Very gentle zoom (0.0002/frame) for ultra-smooth effect
        // At 30fps: ~0.006 zoom/second -> ~2.4% zoom over 4 seconds
        let status = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(img_path)
            .arg("-vf")
            .arg(format!("zoompan=z='min(zoom+0.0002,1.1)':d={}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s={}x{}:fps=30", (duration * 30.0) as i32, w, h))
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

    fn crossfade_clips(
        &self,
        clips: &[std::path::PathBuf],
        output: &Path,
        clip_duration: f64,
    ) -> Result<()> {
        if clips.is_empty() {
            return Err(Error::msg("No clips to crossfade"));
        }
        if clips.len() == 1 {
            std::fs::copy(&clips[0], output)?;
            return Ok(());
        }

        // Build FFmpeg xfade filter chain
        let fade_duration = 0.5;
        let mut filter_parts = Vec::new();
        let mut offset = clip_duration - fade_duration;

        for i in 0..(clips.len() - 1) {
            if i == 0 {
                // First transition: [0:v][1:v]xfade...[v0]
                filter_parts.push(format!(
                    "[0:v][1:v]xfade=transition=fade:duration={}:offset={}[v{}]",
                    fade_duration, offset, i
                ));
            } else {
                // Subsequent: [v{i-1}][{i+1}:v]xfade...[v{i}]
                filter_parts.push(format!(
                    "[v{}][{}:v]xfade=transition=fade:duration={}:offset={}[v{}]",
                    i - 1,
                    i + 1,
                    fade_duration,
                    offset,
                    i
                ));
            }
            offset += clip_duration - fade_duration;
        }

        let filter_complex = filter_parts.join(";");
        let final_output_label = format!("[v{}]", clips.len() - 2);

        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y");

        for clip in clips {
            cmd.arg("-i").arg(clip);
        }

        cmd.arg("-filter_complex")
            .arg(&filter_complex)
            .arg("-map")
            .arg(&final_output_label)
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(output);

        let status = cmd.status()?;
        if !status.success() {
            return Err(Error::msg("Failed to crossfade clips"));
        }
        Ok(())
    }

    pub fn render_subtitle(&self, image: &mut DynamicImage, text: &str) {
        let img_width = image.width() as i32;
        let img_height = image.height() as i32;

        // ROUND 5: Large centered subtitles (font 36)
        let scale = PxScale::from(36.0);
        let char_width = 22.0; // Adjusted for font 36

        // Calculate text width to center it
        let text_width = (text.chars().count() as f32 * char_width) as i32;
        let x_pos = ((img_width - text_width) / 2).max(20);

        // Position at 88% from top (lower for better visibility)
        let y_pos = (img_height as f32 * 0.88) as i32;

        // Draw Text with thick outline for visibility
        let offsets = [
            (-3, -3),
            (-3, 0),
            (-3, 3),
            (0, -3),
            (0, 3),
            (3, -3),
            (3, 0),
            (3, 3),
        ];

        for (ox, oy) in offsets {
            draw_text_mut(
                image,
                Rgba([0u8, 0u8, 0u8, 255u8]), // Black outline
                x_pos + ox,
                y_pos + oy,
                scale,
                &self.font,
                text,
            );
        }

        // Draw white text
        draw_text_mut(
            image,
            Rgba([255u8, 255u8, 255u8, 255u8]),
            x_pos,
            y_pos,
            scale,
            &self.font,
            text,
        );
    }
}
