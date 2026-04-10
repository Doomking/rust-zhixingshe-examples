use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: String,
    pub ai_base_url: String,
    pub ai_api_key: String,
    pub ai_model: String,
    pub zc_url: String,
    pub stt_threshold: f64,
    pub stt_model_path: String,
    pub use_internal_stt: bool,
    pub audio_storage_path: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();

        Self {
            port: env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_string()),
            ai_base_url: env::var("AI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            ai_api_key: env::var("AI_API_KEY").unwrap_or_else(|_| "ollama".to_string()),
            ai_model: env::var("AI_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            zc_url: env::var("ZEROCLAW_URL").unwrap_or_else(|_| "http://127.0.0.1:42617/v1".to_string()),
            stt_threshold: env::var("VAD_THRESHOLD")
                .unwrap_or_else(|_| "2500.0".to_string())
                .parse()
                .unwrap_or(800.0),
            stt_model_path: env::var("STT_MODEL_PATH").unwrap_or_else(|_| "models/ggml-medium.bin".to_string()),
            use_internal_stt: env::var("USE_INTERNAL_STT").unwrap_or_else(|_| "true".to_string()) == "true",
            audio_storage_path: env::var("AUDIO_STORAGE_PATH").unwrap_or_else(|_| "/tmp".to_string()),
        }
    }
}
