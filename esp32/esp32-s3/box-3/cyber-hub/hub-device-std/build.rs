use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    // Load `.env` from crate root (not only CWD) so voice + WiFi vars resolve reliably.
    let env_path = manifest_dir.join(".env");
    let _ = dotenvy::from_filename(&env_path);

    // Load .env file and emit cargo instructions for each variable
    if let Ok(iter) = dotenvy::from_path_iter(&env_path) {
        for item in iter {
            if let Ok((key, value)) = item {
                println!("cargo:rustc-env={}={}", key, value);
            }
        }
    }

    generate_voice_pcm_assets(&manifest_dir, &env_path);

    embuild::espidf::sysenv::output();

    // --- Automated Model Packing ---
    // This logic discovers the esp-sr component in build artifacts and runs the packing script
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "xtensa" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let target_dir = std::path::Path::new(&manifest_dir).join("target");
        
        if let Ok(entries) = std::fs::read_dir(target_dir.join("xtensa-esp32s3-espidf/release/build")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.file_name().unwrap().to_str().unwrap().starts_with("esp-idf-sys-") {
                    let sr_path = path.join("out/managed_components/espressif__esp-sr");
                    if sr_path.exists() {
                        let pack_script = sr_path.join("model/pack_model.py");
                        let model_target = sr_path.join("model/target");
                        let output_bin = std::path::Path::new(&manifest_dir).join("srmodels.bin");

                        if pack_script.exists() && model_target.exists() {
                            println!("cargo:warning=Auto-packing SR models from: {:?}", model_target);
                            let _ = std::process::Command::new("python3")
                                .arg(&pack_script)
                                .arg("-m")
                                .arg(&model_target)
                                .arg("-o")
                                .arg(&output_bin)
                                .status();
                        }
                        break;
                    }
                }
            }
        }
    }
}

/// 从 `.env` 中的 `CYBER_HUB_VOICE_WAKE` / `CYBER_HUB_VOICE_DONE` 生成 `assets/*.pcm`（16 kHz mono s16le）。
/// 需要本机构建机安装 `say`（macOS）与 `ffmpeg`。设置 `CYBER_HUB_VOICE_SKIP_BUILD=1` 可跳过（沿用已有 pcm）。
fn generate_voice_pcm_assets(manifest_dir: &Path, env_path: &Path) {
    println!("cargo:rerun-if-changed={}", env_path.display());
    println!("cargo:rerun-if-env-changed=CYBER_HUB_VOICE_WAKE");
    println!("cargo:rerun-if-env-changed=CYBER_HUB_VOICE_DONE");
    println!("cargo:rerun-if-env-changed=CYBER_HUB_VOICE_SKIP_BUILD");

    if std::env::var("CYBER_HUB_VOICE_SKIP_BUILD").as_deref() == Ok("1") {
        let assets = manifest_dir.join("assets");
        let wake_out = assets.join("wake.pcm");
        let done_out = assets.join("done.pcm");
        if !(wake_out.exists() && done_out.exists()) {
            panic!(
                "CYBER_HUB_VOICE_SKIP_BUILD=1 but missing {:?} or {:?}; generate them once or unset SKIP.",
                wake_out, done_out
            );
        }
        println!("cargo:warning=Voice PCM: skipped (CYBER_HUB_VOICE_SKIP_BUILD=1)");
        return;
    }

    let wake_text = std::env::var("CYBER_HUB_VOICE_WAKE").unwrap_or_else(|_| "我在".to_string());
    let done_text = std::env::var("CYBER_HUB_VOICE_DONE").unwrap_or_else(|_| "好了".to_string());

    let assets = manifest_dir.join("assets");
    if let Err(e) = std::fs::create_dir_all(&assets) {
        println!(
            "cargo:warning=Voice PCM: could not create assets dir: {}",
            e
        );
        return;
    }

    let wake_out = assets.join("wake.pcm");
    let done_out = assets.join("done.pcm");

    if !tool_available("say") || !tool_available("ffmpeg") {
        if wake_out.exists() && done_out.exists() {
            println!("cargo:warning=Voice PCM: `say` or `ffmpeg` not found; using existing wake.pcm / done.pcm");
        } else {
            panic!(
                "Voice PCM: need `say` and `ffmpeg` on PATH to generate assets/wake.pcm and done.pcm, \
                 or set CYBER_HUB_VOICE_SKIP_BUILD=1 and add those files manually."
            );
        }
        return;
    }

    for (label, text, out) in [
        ("wake", wake_text.as_str(), wake_out.as_path()),
        ("done", done_text.as_str(), done_out.as_path()),
    ] {
        if let Err(e) = pcm_from_say_macos(text, out) {
            panic!("Voice PCM ({}): failed to generate {:?}: {}", label, out, e);
        }
        println!(
            "cargo:warning=Voice PCM: generated {} ({})",
            out.display(),
            label
        );
    }
}

fn tool_available(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pcm_from_say_macos(text: &str, out_pcm: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!(
        "cyber_hub_voice_{}.wav",
        out_pcm
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("clip")
    ));

    let st = Command::new("say")
        .arg("-o")
        .arg(&tmp)
        .arg("--data-format=I16@16000")
        .arg(text)
        .status()
        .map_err(|e| format!("say: {}", e))?;
    if !st.success() {
        return Err("say exited non-zero".into());
    }

    let st = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(&tmp)
        .arg("-f")
        .arg("s16le")
        .arg("-acodec")
        .arg("pcm_s16le")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg(out_pcm)
        .status()
        .map_err(|e| format!("ffmpeg: {}", e))?;
    let _ = std::fs::remove_file(&tmp);
    if !st.success() {
        return Err("ffmpeg exited non-zero".into());
    }
    Ok(())
}
