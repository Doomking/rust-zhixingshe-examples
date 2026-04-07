use anyhow::Result;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::{CreateTranscriptionRequestArgs, CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs};
use tracing::{info, warn};
use crate::config::AppConfig;

pub struct AiProcessor {
    stt_client: Client<OpenAIConfig>,
    zc_client: Client<OpenAIConfig>,
    model: String,
}

impl AiProcessor {
    pub fn new(config: &AppConfig) -> Self {
        let stt_config = OpenAIConfig::new()
            .with_api_base(&config.ai_base_url)
            .with_api_key(&config.ai_api_key);
        let stt_client = Client::with_config(stt_config);

        let zc_config = OpenAIConfig::new()
            .with_api_base(&config.zc_url)
            .with_api_key("zeroclaw");
        let zc_client = Client::with_config(zc_config);

        Self {
            stt_client,
            zc_client,
            model: config.ai_model.clone(),
        }
    }

    pub async fn process_utterance(&self, wav_path: String) -> Result<()> {
        info!("[STT] Starting transcription for {}...", wav_path);

        let request = CreateTranscriptionRequestArgs::default()
            .file(wav_path.clone())
            .model("whisper-1")
            .build()?;

        let response = self.stt_client.audio().transcribe(request).await?;
        let text = response.text.trim();
        
        if text.is_empty() {
            warn!("[STT] Empty transcription result. Ignoring.");
            return Ok(());
        }

        println!("\x1b[35m[STT] Recognition Result: \"{}\"\x1b[0m", text);

        // 唤醒词过滤器
        let keywords = ["知行社", "cyberhub", "cyber-hub", "你好", "小知"];
        let mut recognized_command = "";
        
        for kw in keywords {
            if let Some(pos) = text.to_lowercase().find(kw) {
                recognized_command = &text[pos + kw.len()..].trim();
                recognized_command = recognized_command.trim_start_matches(|c: char| c == ',' || c == '，' || c == ' ' || c == '。');
                break;
            }
        }

        if recognized_command.is_empty() {
            info!("[GATEKEEPER] No wake word detected or no command followed.");
            return Ok(());
        }

        info!("\x1b[32;1m[WAKE] Wake word detected! Command: \"{}\"\x1b[0m", recognized_command);

        // 转发给 ZeroClaw
        let zc_request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages([ChatCompletionRequestUserMessageArgs::default()
                .content(recognized_command)
                .build()?
                .into()])
            .build()?;

        let zc_response = self.zc_client.chat().create(zc_request).await?;
        if let Some(choice) = zc_response.choices.first() {
            if let Some(content) = &choice.message.content {
                println!("\x1b[36m[ZeroClaw] Agent Response: \"{}\"\x1b[0m", content);
            }
        }

        Ok(())
    }
}
