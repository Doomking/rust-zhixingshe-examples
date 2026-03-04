use anyhow::{Error, Result};
use msedge_tts::tts::client::connect_async;
use msedge_tts::tts::SpeechConfig;
use msedge_tts::voice::get_voices_list_async;
use std::path::Path;

pub struct SoundEngineer {}

impl SoundEngineer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn construct_soundtrack(
        &self,
        storyboard: &mut crate::director::Storyboard,
    ) -> Result<()> {
        let output_dir = std::path::Path::new("output/audio");
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        // Fetch voices once to avoid repetition (though construct_soundtrack calls record_voice multiple times)
        // Optimization: We could fetch once and pass to record_voice, but for now kept simple inside record_voice.
        // Actually, let's fetch here to avoid network spam.
        let voices = get_voices_list_async()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch voices: {:?}", e)))?;

        let target_voice = voices
            .iter()
            .find(|v| v.short_name.as_deref() == Some("zh-CN-XiaoxiaoNeural"))
            .or_else(|| {
                voices
                    .iter()
                    .find(|v| v.short_name.as_deref().unwrap_or("").contains("Xiaoxiao"))
            })
            .or_else(|| {
                voices
                    .iter()
                    .find(|v| v.short_name.as_deref().unwrap_or("").starts_with("zh-CN"))
            })
            .ok_or_else(|| Error::msg("No suitable Chinese voice found"))?;

        for shot in &mut storyboard.shots {
            let filename = format!("voice_{}.mp3", shot.id);
            let path = output_dir.join(filename);

            if shot.dialogue.trim().is_empty() {
                println!("🔇 Skipping audio for silent shot: {}", shot.panel_name);
                shot.audio_path = None;
                shot.duration = 3.0; // Default duration for silent panels
                continue;
            }

            self.record_voice(&shot.dialogue, target_voice, &path)
                .await?;

            // Note: get_duration uses ffprobe, which works for mp3.
            let duration = self.get_duration(&path).unwrap_or(3.0);
            shot.duration = duration;
            shot.audio_path = Some(
                path.canonicalize()
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string(),
            );
        }
        Ok(())
    }

    pub async fn record_voice(
        &self,
        text: &str,
        voice: &msedge_tts::voice::Voice,
        output_path: &Path,
    ) -> Result<()> {
        // Rust Edge-TTS (msedge-tts)
        // connect_async() takes no arguments
        let mut client = connect_async()
            .await
            .map_err(|e| Error::msg(format!("Failed to connect to Edge TTS: {:?}", e)))?;

        let config = SpeechConfig::from(voice);

        // synthesize takes text and config
        // synthesize takes text and config
        println!(
            "🎤 Synthesizing audio for: '{}' (Voice: {:?})",
            text, voice.short_name
        );

        let audio = client
            .synthesize(text, &config)
            .await
            .map_err(|e| Error::msg(format!("Failed to synthesize speech: {:?}", e)))?;

        println!("✅ Synthesized {} bytes of audio", audio.audio_bytes.len());

        if audio.audio_bytes.is_empty() {
            return Err(Error::msg("Synthesized audio is empty!"));
        }

        std::fs::write(output_path, &audio.audio_bytes)?;
        Ok(())
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
