use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let env_path = manifest_dir.join(".env");
    let _ = dotenvy::from_filename(&env_path);

    if let Ok(iter) = dotenvy::from_path_iter(&env_path) {
        for item in iter {
            if let Ok((key, value)) = item {
                println!("cargo:rustc-env={}={}", key, value);
            }
        }
    }

    let sdkconfig_defaults = manifest_dir.join("sdkconfig.defaults");
    std::env::set_var("ESP_IDF_SDKCONFIG_DEFAULTS", sdkconfig_defaults.to_str().unwrap());

    embuild::espidf::sysenv::output();

    let target = std::env::var("TARGET").unwrap_or_default();
    let mcu = if target.contains("esp32s3") {
        "esp32s3"
    } else if target.contains("esp32s2") {
        "esp32s2"
    } else if target.contains("esp32c3") {
        "esp32c3"
    } else if target.contains("esp32c6") {
        "esp32c6"
    } else {
        "esp32"
    };

    println!(
        "cargo:rustc-link-search=native={}/components/esp-adf-libs/esp_new_jpeg/lib/{}",
        manifest_dir.display(),
        mcu
    );
    println!("cargo:rustc-link-lib=static=esp_new_jpeg");
}
