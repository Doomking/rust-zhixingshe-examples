use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // 1. 定义路径
    let webots_home = "/Applications/Webots.app/Contents";
    let lib_path = format!("{}/lib/controller", webots_home);
    let include_path = format!("{}/include/controller/c/", webots_home);
    
    // [FIX] 新增：定义 Webots App 的根目录 (/Applications/Webots.app)
    // 也就是 webots_home 的上一级
    let webots_app_root = "/Applications/Webots.app";

    // 2. 告诉 Cargo 如何链接库文件 (这部分是为了编译通过)
    println!("cargo:rustc-link-search=native={}", lib_path);
    println!("cargo:rustc-link-lib=dylib=Controller");

    // 3. [FIX] 关键修正：针对 macOS 强制注入正确的 rpath
    // 原来的写法会导致路径重复，现在我们需要指向 App 根目录，
    // 以便匹配 "@rpath/Contents/lib/controller/libController.dylib"
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", webots_app_root);
    
    // 保险起见，原本的 lib_path 也可以保留，但 webots_app_root 是必须的
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path);

    // --- 获取 macOS SDK 路径 ---
    let sdk_path = String::from_utf8(
        Command::new("xcrun")
            .args(&["--show-sdk-path"])
            .output()
            .expect("failed to get sdk path")
            .stdout,
    ).unwrap().trim().to_string();

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_path))
        .clang_arg(format!("-isysroot{}", sdk_path)) 
        .clang_arg("-target")
        .clang_arg("arm64-apple-macos")
        .allowlist_function("wb_.*")
        .allowlist_type("Wb.*")
        .allowlist_var("WB_.*")
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}