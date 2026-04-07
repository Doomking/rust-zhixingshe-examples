use anyhow::{Result, Context};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::{info, warn};

use std::sync::Arc;
use tokio::sync::Mutex;
use once_cell::sync::Lazy;

static DOWNLOAD_LOCK: Lazy<Arc<Mutex<()>>> = Lazy::new(|| Arc::new(Mutex::new(())));

pub async fn download_model_if_needed(model_path: &str) -> Result<()> {
    // 获取全局下载锁，防止并发写入损坏模型
    let _guard = DOWNLOAD_LOCK.lock().await;

    let path = Path::new(model_path);
    
    // 校验已有文件的完整性：如果 Medium 文件太小（比如 < 1GB），视为损坏
    if path.exists() {
        let metadata = std::fs::metadata(path)?;
        let size_mb = metadata.len() / (1024 * 1024);
        
        // 启发式校验：Medium 模型至少应该有 1.4GB 以上
        if model_path.contains("medium") && size_mb < 1400 {
            warn!("[DOWNLOAD] Existing Medium model file is too small ({}MB). Likely corrupt. Deleting...", size_mb);
            let _ = std::fs::remove_file(path);
        } else {
            info!("[DOWNLOAD] Model file found and size looks valid: {} ({}MB).", model_path, size_mb);
            return Ok(());
        }
    }

    warn!("[DOWNLOAD] Model missing or corrupted! Starting fresh download...");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create models directory")?;
    }
    
    let model_filename = path.file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("ggml-base.bin");

    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", model_filename);
    let tmp_path = format!("{}.tmp", model_path);

    let response = reqwest::get(url).await.context("Failed to connect to model host")?;
    let total_size = response.content_length().context("Failed to get content length")?;

    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
        .progress_chars("#>-"));
    pb.set_message(format!("Downloading {}", model_path));

    let mut file = File::create(&tmp_path).context("Failed to create temporary model file")?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.context("Error while downloading chunk")?;
        file.write_all(&chunk).context("Error while writing to file")?;
        downloaded = std::cmp::min(downloaded + (chunk.len() as u64), total_size);
        pb.set_position(downloaded);
    }

    pb.finish_with_message("Download complete!");
    
    // 原子替换：确保只有完整下载的文件才会出现
    std::fs::rename(tmp_path, model_path).context("Failed to rename temporary model file")?;
    info!("[DOWNLOAD] Successfully deployed model: {}", model_path);
    Ok(())
}
