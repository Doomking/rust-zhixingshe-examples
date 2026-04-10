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
}
