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
    /// MJPEG 编码质量: 越小越好 (ffmpeg -q:v 参数)
    pub jpeg_q: u8,
    pub audio_sample_rate: u32,
    pub audio_chunk_bytes: usize,
    pub debug_save_first_frames: usize,
    pub debug_save_audio_seconds: u32,
}

/// 启动视频处理任务: 调用 ffmpeg 将 MP4 实时转码为 MJPEG 序列
pub async fn spawn_video_task(
    cfg: MediaConfig,
    tx: mpsc::Sender<(FrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fps = cfg.video_fps.max(1);
    // 构造 ffmpeg 命令:
    // 1. -stream_loop -1: 循环播放
    // 2. -vf scale...: 缩放并居中补黑边，同时强制输出指定 FPS
    // 3. -c:v mjpeg: 使用 MJPEG 编码
    // 4. -f image2pipe: 将 JPEG 帧序列通过管道输出
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
        .arg("-f")
        .arg("image2pipe")
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdout = child.stdout.take().ok_or("ffmpeg stdout missing")?;

    let mut buf = Vec::<u8>::new();
    let mut frame_idx: u64 = 0;
    let mut saved: usize = 0;

    // JPEG 帧的起始 (SOI) 和结束 (EOI) 标志
    const SOI: [u8; 2] = [0xFF, 0xD8];
    const EOI: [u8; 2] = [0xFF, 0xD9];

    let start_instant = std::time::Instant::now();

    loop {
        let mut chunk = [0u8; 8192];
        let n = stdout.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);

        // 从字节流中解析出完整的 JPEG 帧
        loop {
            let soi_pos = find_marker(&buf, SOI, 0);
            let Some(soi_pos) = soi_pos else { break };
            if soi_pos > 0 {
                buf.drain(0..soi_pos);
            }
            if buf.len() < 4 {
                break;
            }
            let eoi_pos = find_marker(&buf, EOI, 2);
            let Some(eoi_pos) = eoi_pos else { break };

            if eoi_pos + 2 > buf.len() {
                break;
            }
            let frame = buf.drain(0..eoi_pos + 2).collect::<Vec<u8>>();

            // 计算该帧应当出现的时间戳
            let timestamp_ms = ((frame_idx * 1000) / fps as u64) as u32;
            frame_idx += 1;

            // 调试用: 保存前几帧到本地
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

            // ---- 精确节流逻辑 ----
            // 如果推流速度超过了真实播放速度，就休眠等待，防止 TCP 缓冲区阻塞
            let target_time = start_instant + std::time::Duration::from_millis(timestamp_ms as u64);
            let now = std::time::Instant::now();
            if now < target_time {
                tokio::time::sleep(target_time - now).await;
            }

            if tx.send((header, frame)).await.is_err() {
                let _ = child.kill().await;
                return Ok(());
            }
        }
    }

    let _ = child.kill().await;
    Ok(())
}

/// 启动音频处理任务: 调用 ffmpeg 提取单声道 PCM 数据
pub async fn spawn_audio_task(
    cfg: MediaConfig,
    tx: mpsc::Sender<(FrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 检查视频文件是否包含音频流
    let has_audio = has_audio_stream(&cfg.mp4_path).await?;
    if !has_audio {
        eprintln!("[media] 未检测到音频流，跳过音频任务");
        return Ok(());
    }

    let chunk_bytes = cfg.audio_chunk_bytes;
    let sr = cfg.audio_sample_rate;
    if chunk_bytes % 2 != 0 {
        return Err("音频块大小必须是偶数 (s16le)".into());
    }
    let chunk_samples = (chunk_bytes / 2) as u32;
    let debug_audio_samples = cfg.audio_sample_rate.saturating_mul(cfg.debug_save_audio_seconds);
    let debug_audio_bytes = debug_audio_samples as usize * 2;

    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-stream_loop")
        .arg("-1")
        .arg("-i")
        .arg(&cfg.mp4_path)
        .arg("-map")
        .arg("0:a:0") // 仅选择第一个音频流
        .arg("-ac")
        .arg("1") // 转换为单声道
        .arg("-ar")
        .arg(sr.to_string()) // 采样率 16000
        .arg("-c:a")
        .arg("pcm_s16le") // 原始 PCM 编码
        .arg("-f")
        .arg("s16le")
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdout = child.stdout.take().ok_or("ffmpeg stdout missing")?;
    let mut buf = vec![0u8; chunk_bytes];
    let mut chunk_idx: u64 = 0;
    let start_instant = std::time::Instant::now();

    loop {
        // 按照指定的块大小 (通常是 2048 字节) 读取音频
        let read_res = stdout.read_exact(&mut buf).await;
        if let Err(e) = read_res {
            let _ = child.kill().await;
            return Err(e.into());
        }

        // 计算该音频块应当对应的时间戳
        let timestamp_ms =
            ((chunk_idx * chunk_samples as u64 * 1000) / sr as u64) as u32;
        chunk_idx += 1;

        let header = FrameHeader {
            frame_type: FrameType::AudioPcm,
            timestamp_ms,
            payload_len: buf.len() as u32,
        };

        // ---- 音频精确节流 ----
        let target_time = start_instant + std::time::Duration::from_millis(timestamp_ms as u64);
        let now = std::time::Instant::now();
        if now < target_time {
            tokio::time::sleep(target_time - now).await;
        }

        if tx.send((header, buf.clone())).await.is_err() {
            let _ = child.kill().await;
            return Ok(());
        }
    }
}

/// 在字节流中查找特定的标志位 (如 JPEG 的 SOI/EOI)
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

/// 使用 ffprobe 快速检查文件中是否存在音频流
async fn has_audio_stream(mp4_path: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
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
