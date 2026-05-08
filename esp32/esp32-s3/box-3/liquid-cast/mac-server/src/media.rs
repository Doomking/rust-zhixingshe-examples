use crate::protocol::{FrameHeader, FrameType};
use std::path::Path;
use std::io::Write;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct MediaConfig {
    pub mp4_path: String,
    pub video_width: u32,
    pub video_height: u32,
    pub video_fps: u32,
    /// MJPEG encoder quality: lower is better (ffmpeg -q:v semantics).
    pub jpeg_q: u8,
    pub audio_sample_rate: u32,
    pub audio_chunk_bytes: usize,
    pub debug_save_first_frames: usize,
    pub debug_save_audio_seconds: u32,
}

pub async fn spawn_video_task(
    cfg: MediaConfig,
    tx: mpsc::Sender<(FrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fps = cfg.video_fps.max(1);
    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-stream_loop")
        .arg("-1")
        .arg("-i")
        .arg(&cfg.mp4_path)
        .arg("-vf")
        .arg(format!(
            "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:color=black,fps={}",
            cfg.video_width, cfg.video_height, cfg.video_width, cfg.video_height, fps
        ))
        .arg("-an")
        .arg("-c:v")
        .arg("mjpeg")
        .arg("-q:v")
        .arg(cfg.jpeg_q.to_string())
        // image2pipe gives a concatenated MJPEG byte stream (SOI..EOI blocks).
        .arg("-f")
        .arg("image2pipe")
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdout = child.stdout.take().ok_or("ffmpeg stdout missing")?;

    let mut buf = Vec::<u8>::new();
    let mut frame_idx: u64 = 0;
    let mut saved: usize = 0;

    const SOI: [u8; 2] = [0xFF, 0xD8];
    const EOI: [u8; 2] = [0xFF, 0xD9];

    loop {
        let mut chunk = [0u8; 8192];
        let n = stdout.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);

        loop {
            let soi_pos = find_marker(&buf, SOI, 0);
            let Some(soi_pos) = soi_pos else { break };
            // Discard everything before SOI.
            if soi_pos > 0 {
                buf.drain(0..soi_pos);
            }
            // Now SOI is at 0.
            if buf.len() < 4 {
                break;
            }
            let eoi_pos = find_marker(&buf, EOI, 2);
            let Some(eoi_pos) = eoi_pos else { break };

            // Extract inclusive EOI.
            if eoi_pos + 2 > buf.len() {
                break;
            }
            let frame = buf.drain(0..eoi_pos + 2).collect::<Vec<u8>>();

            let timestamp_ms = ((frame_idx * 1000) / fps as u64) as u32;
            frame_idx += 1;

            if saved < cfg.debug_save_first_frames {
                let base_dir = Path::new(&cfg.mp4_path)
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
                let out_dir = base_dir.join("debug_frames");
                let _ = std::fs::create_dir_all(&out_dir);
                let out_path = out_dir.join(format!("frame_{:05}.jpg", frame_idx));
                let _ = std::fs::write(&out_path, &frame);
                saved += 1;
            }

            let header = FrameHeader {
                frame_type: FrameType::VideoJpeg,
                timestamp_ms,
                payload_len: frame.len() as u32,
            };
            if tx.send((header, frame)).await.is_err() {
                let _ = child.kill().await;
                return Ok(());
            }
        }
    }

    let _ = child.kill().await;
    Ok(())
}

pub async fn spawn_audio_task(
    cfg: MediaConfig,
    tx: mpsc::Sender<(FrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Some MP4 files don't contain an audio stream; ffmpeg would error with:
    // "Output file does not contain any stream". Detect this early.
    let has_audio = has_audio_stream(&cfg.mp4_path).await?;
    if !has_audio {
        eprintln!(
            "[media] No audio stream detected in `{}`; skipping audio task.",
            cfg.mp4_path
        );
        return Ok(());
    }

    let chunk_bytes = cfg.audio_chunk_bytes;
    let sr = cfg.audio_sample_rate;
    if chunk_bytes % 2 != 0 {
        return Err("audio_chunk_bytes must be even (s16le)".into());
    }
    let chunk_samples = (chunk_bytes / 2) as u32;
    let debug_audio_samples = cfg.audio_sample_rate.saturating_mul(cfg.debug_save_audio_seconds);
    let debug_audio_bytes = debug_audio_samples as usize * 2; // mono s16le

    let base_dir = Path::new(&cfg.mp4_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let debug_dir = base_dir.join("debug_frames");
    let _ = std::fs::create_dir_all(&debug_dir);
    let debug_pcm_path = debug_dir.join("debug_audio.pcm");
    let debug_wav_path = debug_dir.join("debug_audio.wav");

    let mut debug_pcm_file = std::fs::File::create(&debug_pcm_path)?;
    let mut debug_written: usize = 0;

    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-stream_loop")
        .arg("-1")
        .arg("-i")
        .arg(&cfg.mp4_path)
        // Select the first audio stream explicitly.
        .arg("-map")
        .arg("0:a:0")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg(sr.to_string())
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg("-f")
        .arg("s16le")
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdout = child.stdout.take().ok_or("ffmpeg stdout missing")?;
    let mut buf = vec![0u8; chunk_bytes];
    let mut chunk_idx: u64 = 0;

    loop {
        let read_res = stdout.read_exact(&mut buf).await;
        if let Err(e) = read_res {
            // Most likely EOF / child ended.
            let _ = child.kill().await;
            return Err(e.into());
        }

        let timestamp_ms =
            ((chunk_idx * chunk_samples as u64 * 1000) / sr as u64) as u32;
        chunk_idx += 1;

        if debug_written < debug_audio_bytes {
            let remaining = debug_audio_bytes - debug_written;
            let to_write = remaining.min(buf.len());
            debug_pcm_file.write_all(&buf[..to_write])?;
            debug_written += to_write;
            // Once enough samples are dumped, convert to wav and stop further debug.
            if debug_written >= debug_audio_bytes {
                let _ = tokio::process::Command::new("ffmpeg")
                    .arg("-y")
                    .arg("-loglevel")
                    .arg("error")
                    .arg("-f")
                    .arg("s16le")
                    .arg("-ar")
                    .arg(sr.to_string())
                    .arg("-ac")
                    .arg("1")
                    .arg("-i")
                    .arg(&debug_pcm_path)
                    .arg(&debug_wav_path)
                    .status()
                    .await;
            }
        }

        let header = FrameHeader {
            frame_type: FrameType::AudioPcm,
            timestamp_ms,
            payload_len: buf.len() as u32,
        };

        if tx.send((header, buf.clone())).await.is_err() {
            let _ = child.kill().await;
            return Ok(());
        }
    }
}

fn find_marker(buf: &[u8], marker: [u8; 2], start: usize) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in start..(buf.len() - 1) {
        if buf[i] == marker[0] && buf[i + 1] == marker[1] {
            return Some(i);
        }
    }
    None
}

async fn has_audio_stream(mp4_path: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    // We only need to know whether an audio stream exists; this is a fast ffprobe check.
    let out = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=index")
        .arg("-of")
        .arg("csv=p=0")
        .arg(mp4_path)
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        Err(e) => return Err(format!("ffprobe failed: {e}").into()),
    };

    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

