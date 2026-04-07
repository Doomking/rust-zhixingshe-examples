pub mod downloader;
pub mod local_stt;

use anyhow::Result;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::{CreateTranscriptionRequestArgs, CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs};
use tracing::{info, warn, error};
use crate::config::AppConfig;
use self::local_stt::LocalStt;
use self::downloader::download_model_if_needed;

pub struct AiProcessor {
    stt_client: Client<OpenAIConfig>,
    zc_client: Client<OpenAIConfig>,
    model: String,
    local_stt: Option<std::sync::Arc<LocalStt>>,
    use_internal: bool,
    last_wake_time: std::sync::Mutex<Option<std::time::Instant>>,
}

impl AiProcessor {
    pub async fn new(config: &AppConfig) -> Self {
        let stt_config = OpenAIConfig::new()
            .with_api_base(&config.ai_base_url)
            .with_api_key(&config.ai_api_key);
        let stt_client = Client::with_config(stt_config);

        let zc_config = OpenAIConfig::new()
            .with_api_base(&config.zc_url)
            .with_api_key("zeroclaw");
        let zc_client = Client::with_config(zc_config);

        let local_stt = if config.use_internal_stt {
            if let Err(e) = download_model_if_needed(&config.stt_model_path).await {
                error!("[AI] Failed to download/check model: {}. Falling back to API mode.", e);
                None
            } else {
                match LocalStt::new(&config.stt_model_path) {
                    Ok(stt) => Some(std::sync::Arc::new(stt)),
                    Err(e) => {
                        error!("[AI] Failed to initialize local STT: {}. Falling back to API mode.", e);
                        None
                    }
                }
            }
        } else {
            None
        };

        Self {
            stt_client,
            zc_client,
            model: config.ai_model.clone(),
            local_stt,
            use_internal: config.use_internal_stt,
            last_wake_time: std::sync::Mutex::new(None),
        }
    }

    pub async fn process_utterance(&self, wav_path: String) -> Result<()> {
        let text = if self.use_internal && self.local_stt.is_some() {
            // 本地推理
            self.process_utterance_internally(&wav_path)?
        } else {
            // API 模式
            self.process_utterance_via_api(&wav_path).await?
        };

        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }

        println!("\x1b[35m[STT] Recognition Result: \"{}\"\x1b[0m", text);

        // 2. 唤醒词与指令判定 (已移除纠偏层，直接使用原始文本)
        let keywords = ["知行社", "cyberhub", "你好", "小知", "小智"];
        let mut wake_word_found = false;
        let mut command = String::new();

        for kw in keywords {
            if let Some(pos) = text.to_lowercase().find(kw) {
                wake_word_found = true;
                let raw_cmd = &text[pos + kw.len()..].trim();
                command = raw_cmd.trim_start_matches(|c: char| c == ',' || c == '，' || c == ' ' || c == '。').to_string();
                break;
            }
        }

        // 3. 状态机逻辑：判断 8 秒待命窗口并更新状态 (锁的作用域仅限此块)
        let final_command: String = {
            let now = std::time::Instant::now();
            let mut last_wake = self.last_wake_time.lock().unwrap();
            
            let is_in_window = if let Some(t) = *last_wake {
                now.duration_since(t) < std::time::Duration::from_secs(8)
            } else {
                false
            };

            if wake_word_found {
                *last_wake = Some(now); // 刷新唤醒时间
                if command.is_empty() {
                    info!("\x1b[32;1m[WAKE] Hello! I'm listening...\x1b[0m");
                    return Ok(());
                }
                command // command 已经是 String
            } else if is_in_window {
                // 在待命状态下，整句话都视为指令
                text.to_string()
            } else {
                info!("[GATEKEEPER] No wake word detected.");
                return Ok(());
            }
        }; // 这里 guard 会被自动 drop

        info!("\x1b[32;1m[WAKE] Command accepted: \"{}\"\x1b[0m", final_command);

        // 4. 发送指令 (此处不再持有锁，可以安全 await)
        let zc_request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages([ChatCompletionRequestUserMessageArgs::default()
                .content(final_command)
                .build()?
                .into()])
            .build()?;

        // 尝试发送到 ZeroClaw，如果失败则静默提示
        match self.zc_client.chat().create(zc_request).await {
            Ok(zc_response) => {
                if let Some(choice) = zc_response.choices.first() {
                    if let Some(content) = &choice.message.content {
                        println!("\x1b[36m[ZeroClaw] Agent Response: \"{}\"\x1b[0m", content);
                    }
                }
            }
            Err(_) => {
                info!("[GATEKEEPER] ZeroClaw relay skipped (Is it running on port 42617?)");
            }
        }

        Ok(())
    }

    fn process_utterance_internally(&self, wav_path: &str) -> Result<String> {
        let mut reader = hound::WavReader::open(wav_path)?;
        let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap_or(0)).collect();
        self.local_stt.as_ref().unwrap().transcribe(&samples)
    }

    async fn process_utterance_via_api(&self, wav_path: &str) -> Result<String> {
        info!("[STT] Starting transcription for {}...", wav_path);

        let request = CreateTranscriptionRequestArgs::default()
            .file(wav_path.to_string())
            .model("whisper-1")
            .build()?;

        let response = self.stt_client.audio().transcribe(request).await?;
        Ok(response.text)
    }
}
