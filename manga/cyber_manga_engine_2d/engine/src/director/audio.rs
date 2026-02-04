use anyhow::{Error, Result};
use std::path::Path;

pub struct SoundEngineer {}

impl SoundEngineer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn construct_soundtrack(&self, storyboard: &mut crate::director::Storyboard) -> Result<()> {
        let output_dir = std::path::Path::new("output/audio");
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        for shot in &mut storyboard.shots {
            let voice_id = if let Some(c) = storyboard.characters.get(&shot.character) {
                &c.voice_id
            } else {
                "zh-CN-XiaoxiaoNeural"
            };

            let filename = format!("voice_{}.mp3", shot.id);
            let path = output_dir.join(filename);

            self.record_voice(&shot.dialogue, voice_id, &path)?;

            let duration = self.get_duration(&path).unwrap_or(3.0);
            shot.duration = duration;
            shot.audio_path = Some(path.canonicalize().unwrap_or(path));
        }
        Ok(())
    }

    pub fn record_voice(&self, text: &str, voice_id: &str, output_path: &Path) -> Result<()> {
        // Simple Edge-TTS wrapper
        let status = std::process::Command::new("edge-tts")
            .arg("--voice")
            .arg(voice_id)
            .arg("--text")
            .arg(text)
            .arg("--write-media")
            .arg(output_path)
            .status();

        match status {
            Ok(s) if s.success() => Ok(()),
            _ => {
                // Fallback to python module
                let status2 = std::process::Command::new("python3")
                    .arg("-m")
                    .arg("edge_tts")
                    .arg("--voice")
                    .arg(voice_id)
                    .arg("--text")
                    .arg(text)
                    .arg("--write-media")
                    .arg(output_path)
                    .status()?;

                if !status2.success() {
                    return Err(Error::msg("TTS generation failed"));
                }
                Ok(())
            }
        }
    }

    pub fn get_duration(&self, audio_path: &Path) -> Result<f64> {
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
}
