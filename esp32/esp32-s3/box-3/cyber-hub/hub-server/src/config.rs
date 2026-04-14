use std::env;

/// How `hub-server` forwards text to ZeroClaw Gateway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZcRelayMode {
    /// Auto-detect and fallback: ws_chat -> webhook -> openai.
    Auto,
    /// `GET /ws/chat` WebSocket — full agent with tools (`turn_streamed`). Preferred for CyberHub.
    WsChat,
    /// `POST /webhook` — upstream uses **no tools** (`run_gateway_chat_simple`); chat-only.
    Webhook,
    /// OpenAI-shaped `POST …/v1/chat/completions` via `async-openai` (only if Gateway mounts `/v1`).
    Openai,
}

/// Text-to-speech backend selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtsBackend {
    /// Auto-detect runtime capabilities by OS + available binaries.
    Auto,
    /// macOS `say` + `ffmpeg`.
    MacSay,
    /// Piper CLI (cross-platform local TTS).
    Piper,
    /// Disable runtime TTS synthesis (fallback to done cue only).
    None,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: String,
    pub ai_base_url: String,
    pub ai_api_key: String,
    pub ai_model: String,
    pub zc_url: String,
    /// Gateway bearer token when pairing / auth is enabled (not the LLM provider API key).
    pub zc_api_key: String,
    pub zc_relay_mode: ZcRelayMode,
    /// Full URL for `POST /webhook` (include `path_prefix` if configured in ZeroClaw).
    pub zc_webhook_url: String,
    /// Optional second factor if `[channels_config.webhook]` secret is set in ZeroClaw.
    pub zc_webhook_secret: Option<String>,
    /// WebSocket URL without query (e.g. `ws://127.0.0.1:42617/ws/chat`); `token` / `session_id` appended by hub-server.
    pub zc_ws_chat_url: String,
    /// Stable session for multi-turn on BOX-3; `None` = omit (new UUID each connection).
    pub zc_ws_session_id: Option<String>,
    pub zc_ws_session_name: Option<String>,
    /// Whether to synthesize ZeroClaw text reply and stream downlink PCM to BOX-3.
    pub enable_tts_feedback: bool,
    pub tts_backend: TtsBackend,
    /// macOS `say` voice name, e.g. Tingting / Sin-ji / Mei-Jia.
    pub tts_voice: String,
    /// macOS `say -r` speed.
    pub tts_rate: u32,
    /// Truncate long LLM replies before local TTS generation.
    pub tts_max_chars: usize,
    /// Piper binary path when `tts_backend=piper`.
    pub tts_piper_cmd: String,
    /// Piper model path when `tts_backend=piper`.
    pub tts_piper_model: String,
    pub stt_threshold: f64,
    pub stt_model_path: String,
    pub use_internal_stt: bool,
    pub audio_storage_path: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();

        let zc_relay_mode = match env::var("ZEROCLAW_MODE")
            .unwrap_or_else(|_| "auto".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "auto" => ZcRelayMode::Auto,
            "openai" => ZcRelayMode::Openai,
            "webhook" => ZcRelayMode::Webhook,
            "ws" | "ws_chat" | "websocket" => ZcRelayMode::WsChat,
            _ => ZcRelayMode::Auto,
        };

        let zc_ws_session_id = match env::var("ZEROCLAW_WS_SESSION_ID") {
            Ok(s) if s.trim().is_empty() => None,
            Ok(s) => Some(s),
            Err(_) => Some("cyber-hub".to_string()),
        };

        let tts_backend = match env::var("TTS_BACKEND")
            .unwrap_or_else(|_| "auto".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "none" | "off" | "disabled" => TtsBackend::None,
            "piper" => TtsBackend::Piper,
            "auto" => TtsBackend::Auto,
            _ => TtsBackend::MacSay,
        };

        Self {
            port: env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_string()),
            ai_base_url: env::var("AI_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            ai_api_key: env::var("AI_API_KEY").unwrap_or_else(|_| "ollama".to_string()),
            ai_model: env::var("AI_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            zc_url: env::var("ZEROCLAW_URL").unwrap_or_else(|_| "http://127.0.0.1:42617/v1".to_string()),
            zc_api_key: env::var("ZEROCLAW_API_KEY").unwrap_or_else(|_| "zeroclaw".to_string()),
            zc_relay_mode,
            zc_webhook_url: env::var("ZEROCLAW_WEBHOOK_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:42617/webhook".to_string()),
            zc_webhook_secret: env::var("ZEROCLAW_WEBHOOK_SECRET")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            zc_ws_chat_url: env::var("ZEROCLAW_WS_CHAT_URL")
                .unwrap_or_else(|_| "ws://127.0.0.1:42617/ws/chat".to_string()),
            zc_ws_session_id,
            zc_ws_session_name: env::var("ZEROCLAW_WS_SESSION_NAME")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            enable_tts_feedback: env::var("ENABLE_TTS_FEEDBACK")
                .unwrap_or_else(|_| "true".to_string())
                == "true",
            tts_backend,
            tts_voice: env::var("TTS_VOICE").unwrap_or_else(|_| "Tingting".to_string()),
            tts_rate: env::var("TTS_RATE")
                .unwrap_or_else(|_| "180".to_string())
                .parse()
                .unwrap_or(180),
            tts_max_chars: env::var("TTS_MAX_CHARS")
                .unwrap_or_else(|_| "180".to_string())
                .parse()
                .unwrap_or(180),
            tts_piper_cmd: env::var("TTS_PIPER_CMD").unwrap_or_else(|_| "piper".to_string()),
            tts_piper_model: env::var("TTS_PIPER_MODEL").unwrap_or_else(|_| "".to_string()),
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
