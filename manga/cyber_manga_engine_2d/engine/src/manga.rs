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

#[derive(Debug, Clone)]
pub struct MergedPanel {
    pub prompt: String,
    pub dialogues: Vec<(String, String)>, // Vec of (role, dialogue)
}

#[derive(Clone)]
pub struct VideoFrameData {
    pub base_image: DynamicImage,
    pub role: String,
    pub dialogue: String,
    pub bubble_idx: usize,
    pub total_bubbles: usize,
}

impl Script {
    pub fn parse(text: &str) -> Self {
        let mut panels = Vec::new();
        for line in text.lines() {
            if let Some((role, dialogue)) = line.split_once(&[':', '：'][..]) {
                let role = role.trim().to_string();
                let dialogue = dialogue.trim().to_string();

                // Generic prompt engineering based on role name
                // "Role, Studio Ghibli style..." will be constructed later or used here.
                let visual_prompt = format!("{}, Studio Ghibli style, bright colors", role);

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

    /// Merge consecutive panels into MergedPanels (2 dialogues per frame)
    pub fn merge_panels(panels: Vec<Panel>) -> Vec<MergedPanel> {
        let mut merged = Vec::new();
        let mut i = 0;

        while i < panels.len() {
            if i + 1 < panels.len() {
                // Merge two consecutive panels
                let first = &panels[i];
                let second = &panels[i + 1];

                // Create a combined prompt: "Role1 and Role2, Studio Ghibli style..."
                // We use the first panel's style suffix (which is generic now) but combine names
                let combined_prompt = format!(
                    "{} and {}, Studio Ghibli style, bright colors, interaction, cinematic shot",
                    first.role, second.role
                );

                merged.push(MergedPanel {
                    prompt: combined_prompt,
                    dialogues: vec![
                        (first.role.clone(), first.dialogue.clone()),
                        (second.role.clone(), second.dialogue.clone()),
                    ],
                });
                i += 2;
            } else {
                // Single panel remaining
                let panel = &panels[i];
                merged.push(MergedPanel {
                    prompt: panel.prompt.clone(),
                    dialogues: vec![(panel.role.clone(), panel.dialogue.clone())],
                });
                i += 1;
            }
        }

        merged
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

    pub async fn generate_manga(
        &self,
        script_text: &str,
    ) -> Result<(DynamicImage, Vec<Panel>, Option<Vec<u8>>)> {
        let script = Script::parse(script_text);

        // Merge consecutive dialogues into single frames
        let merged_panels = Script::merge_panels(script.panels.clone());
        let mut images = Vec::new();

        for merged in &merged_panels {
            println!(
                "Generating merged panel with {} dialogue(s)",
                merged.dialogues.len()
            );

            // Generate base image using the prompt
            let mut image = self.sd.generate(&merged.prompt, 30, 7.5, None)?;

            // Render all speech bubbles for this merged panel
            for (idx, (role, dialogue)) in merged.dialogues.iter().enumerate() {
                self.render_speech_bubble(&mut image, role, dialogue, idx, merged.dialogues.len());
            }

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
        let mut current_y = 0;
        for img in &images {
            manga_page.copy_from(img, 0, current_y)?;
            current_y += img.height() + spacing;
        }

        // Generate Advanced Video (TTS + ZoomPan)
        let video_data = match self.generate_final_video(&images, &script.panels).await {
            Ok(data) => Some(data),
            Err(e) => {
                println!("Advanced Video generation failed: {}", e);
                None
            }
        };

        Ok((manga_page, script.panels, video_data))
    }

    async fn generate_final_video(
        &self,
        images: &[DynamicImage],
        panels: &[Panel],
    ) -> Result<Vec<u8>> {
        let temp_dir = Path::new("temp_manga_video");
        if temp_dir.exists() {
            std::fs::remove_dir_all(temp_dir)?;
        }
        std::fs::create_dir(temp_dir)?;

        let mut clip_paths = Vec::new();

        for (i, (img, panel)) in images.iter().zip(panels.iter()).enumerate() {
            let img_path = temp_dir.join(format!("frame_{}.png", i));
            let audio_path = temp_dir.join(format!("audio_{}.mp3", i));
            let clip_path = temp_dir.join(format!("clip_{}.mp4", i));

            // 1. Save Image
            img.save(&img_path)?;

            // 2. Generate Audio (TTS)
            self.generate_audio(&panel.role, &panel.dialogue, &audio_path)?;

            // 3. Get Audio Duration
            let duration = self.get_audio_duration(&audio_path).unwrap_or(3.0);

            // 4. Generate Clip (ZoomPan effect)
            self.generate_clip(&img_path, &audio_path, &clip_path, duration)?;

            clip_paths.push(format!("file 'clip_{}.mp4'", i));
        }

        // 5. Concat Clips
        let list_path = temp_dir.join("list.txt");
        std::fs::write(&list_path, clip_paths.join("\n"))?;

        let output_path = temp_dir.join("output.mp4");

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

        let video_bytes = std::fs::read(&output_path)?;
        let _ = std::fs::remove_dir_all(temp_dir);
        Ok(video_bytes)
    }

    fn generate_audio(&self, role: &str, text: &str, output_path: &Path) -> Result<()> {
        // Voice Mapping
        let voice = match role.to_lowercase().as_str() {
            r if r.contains("girl") || r.contains("woman") => "zh-CN-XiaoxiaoNeural",
            r if r.contains("robot") || r.contains("bot") || r.contains("man") => {
                "zh-CN-YunxiNeural"
            }
            _ => "zh-CN-YunjianNeural",
        };

        // Note: edge-tts is a Python CLI tool.
        // Ensure "edge-tts" is in PATH or use "python3 -m edge_tts"
        // Let's try direct "edge-tts" first since we installed it.
        // If it fails, fallback to python module call.
        let status = std::process::Command::new("edge-tts")
            .arg("--voice")
            .arg(voice)
            .arg("--text")
            .arg(text)
            .arg("--write-media")
            .arg(output_path)
            .status();

        match status {
            Ok(s) if s.success() => Ok(()),
            _ => {
                // Fallback: try python3 -m edge_tts
                let status2 = std::process::Command::new("python3")
                    .arg("-m")
                    .arg("edge_tts")
                    .arg("--voice")
                    .arg(voice)
                    .arg("--text")
                    .arg(text)
                    .arg("--write-media")
                    .arg(output_path)
                    .status()?;

                if !status2.success() {
                    return Err(Error::msg(
                        "TTS generation failed (both direct and python module)",
                    ));
                }
                Ok(())
            }
        }
    }

    fn get_audio_duration(&self, audio_path: &Path) -> Result<f64> {
        let output = std::process::Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=duration")
            .arg("-of")
            .arg("default=noprint_wrappers=1:nokey=1")
            .arg(audio_path)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let duration: f64 = stdout.trim().parse()?;
        Ok(duration)
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
            .arg("-vf").arg(format!("zoompan=z='min(zoom+0.0002,1.05)':d={}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s=512x512:fps=30", (video_duration * 30.0) as i32)) 
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

    fn render_speech_bubble(
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

        // 3. Position based on bubble index and total count
        let (x, y) = if total_bubbles == 1 {
            // Single bubble: centered at bottom
            let x = (img_width - bubble_width) / 2;
            let y = img_height - bubble_height - 30;
            (x, y)
        } else if bubble_idx == 0 {
            // First bubble (top): positioned in upper area, alternating left/right
            let x = if bubble_idx % 2 == 0 {
                40 // Left side
            } else {
                img_width - bubble_width - 40 // Right side
            };
            let y = 50; // Top area
            (x, y)
        } else {
            // Second bubble (bottom): positioned in lower area, opposite side
            let x = if bubble_idx % 2 == 0 {
                40 // Left side
            } else {
                img_width - bubble_width - 40 // Right side
            };
            let y = img_height - bubble_height - 50; // Bottom area
            (x, y)
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
