fn main() {
    // Load .env file and emit cargo instructions for each variable
    if let Ok(iter) = dotenvy::dotenv_iter() {
        for item in iter {
            if let Ok((key, value)) = item {
                println!("cargo:rustc-env={}={}", key, value);
            }
        }
    }

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
