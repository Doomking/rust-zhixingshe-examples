use crate::config::TtsBackend;
use tokio::fs;
use tokio::process::Command;
use tracing::warn;

pub struct TtsRequest<'a> {
    pub backend: TtsBackend,
    pub text: &'a str,
    pub max_chars: usize,
    pub voice: &'a str,
    pub rate: u32,
    pub piper_cmd: &'a str,
    pub piper_model: &'a str,
}

pub async fn synthesize_tts_pcm(req: &TtsRequest<'_>) -> Option<Vec<u8>> {
    let mut t = req.text.replace('\n', " ");
    if t.chars().count() > req.max_chars {
        t = t.chars().take(req.max_chars).collect();
    }
    if t.trim().is_empty() {
        return None;
    }

    let backend = resolve_backend(req).await;
    match backend {
        TtsBackend::None => None,
        TtsBackend::MacSay => synth_mac_say(&t, req.voice, req.rate).await,
        TtsBackend::Piper => synth_piper(&t, req.piper_cmd, req.piper_model).await,
        TtsBackend::Auto => None,
    }
}

async fn resolve_backend(req: &TtsRequest<'_>) -> TtsBackend {
    match req.backend {
        TtsBackend::Auto => {
            let os = std::env::consts::OS;
            let has_ffmpeg = command_exists("ffmpeg").await;
            if os == "macos" && has_ffmpeg && command_exists("say").await {
                return TtsBackend::MacSay;
            }
            if has_ffmpeg
                && !req.piper_model.trim().is_empty()
                && command_exists(req.piper_cmd).await
            {
                return TtsBackend::Piper;
            }
            warn!(
                "[TTS] auto detect found no usable backend (os={}, ffmpeg={}, say={}, piper_cmd={}, model_set={})",
                os,
                has_ffmpeg,
                if os == "macos" { command_exists("say").await } else { false },
                command_exists(req.piper_cmd).await,
                !req.piper_model.trim().is_empty()
            );
            TtsBackend::None
        }
        b => b,
    }
}

async fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--help").output().await.is_ok()
}

async fn synth_mac_say(text: &str, voice: &str, rate: u32) -> Option<Vec<u8>> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let aiff = format!("/tmp/cyberhub_tts_{ts}.aiff");
    let pcm = format!("/tmp/cyberhub_tts_{ts}.pcm");

    let say = Command::new("say")
        .arg("-v")
        .arg(voice)
        .arg("-r")
        .arg(rate.to_string())
        .arg("-o")
        .arg(&aiff)
        .arg(text)
        .output()
        .await;
    match say {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            warn!("[TTS] `say` failed: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        Err(e) => {
            warn!("[TTS] spawn `say` failed: {e}");
            return None;
        }
    }

    let ff = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(&aiff)
        .arg("-f")
        .arg("s16le")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg(&pcm)
        .output()
        .await;
    match ff {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            warn!(
                "[TTS] `ffmpeg` convert failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let _ = fs::remove_file(&aiff).await;
            return None;
        }
        Err(e) => {
            warn!("[TTS] spawn `ffmpeg` failed: {e}");
            let _ = fs::remove_file(&aiff).await;
            return None;
        }
    }

    let bytes = match fs::read(&pcm).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            warn!("[TTS] generated empty PCM");
            let _ = fs::remove_file(&aiff).await;
            let _ = fs::remove_file(&pcm).await;
            return None;
        }
        Err(e) => {
            warn!("[TTS] read PCM failed: {e}");
            let _ = fs::remove_file(&aiff).await;
            let _ = fs::remove_file(&pcm).await;
            return None;
        }
    };

    let _ = fs::remove_file(&aiff).await;
    let _ = fs::remove_file(&pcm).await;
    Some(bytes)
}

async fn synth_piper(text: &str, piper_cmd: &str, piper_model: &str) -> Option<Vec<u8>> {
    if piper_model.trim().is_empty() {
        warn!("[TTS] piper backend selected but TTS_PIPER_MODEL is empty");
        return None;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let wav = format!("/tmp/cyberhub_tts_{ts}.wav");
    let pcm = format!("/tmp/cyberhub_tts_{ts}.pcm");

    let piper = Command::new(piper_cmd)
        .arg("--model")
        .arg(piper_model)
        .arg("--output_file")
        .arg(&wav)
        .arg("--text")
        .arg(text)
        .output()
        .await;
    match piper {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            warn!("[TTS] `piper` failed: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        Err(e) => {
            warn!("[TTS] spawn `piper` failed: {e}");
            return None;
        }
    }

    let ff = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(&wav)
        .arg("-f")
        .arg("s16le")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg(&pcm)
        .output()
        .await;
    match ff {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            warn!(
                "[TTS] `ffmpeg` convert failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let _ = fs::remove_file(&wav).await;
            return None;
        }
        Err(e) => {
            warn!("[TTS] spawn `ffmpeg` failed: {e}");
            let _ = fs::remove_file(&wav).await;
            return None;
        }
    }

    let bytes = match fs::read(&pcm).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            warn!("[TTS] generated empty PCM");
            let _ = fs::remove_file(&wav).await;
            let _ = fs::remove_file(&pcm).await;
            return None;
        }
        Err(e) => {
            warn!("[TTS] read PCM failed: {e}");
            let _ = fs::remove_file(&wav).await;
            let _ = fs::remove_file(&pcm).await;
            return None;
        }
    };

    let _ = fs::remove_file(&wav).await;
    let _ = fs::remove_file(&pcm).await;
    Some(bytes)
}
