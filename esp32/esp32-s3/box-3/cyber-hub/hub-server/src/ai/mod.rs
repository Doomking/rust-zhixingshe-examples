pub mod downloader;
pub mod local_stt;

use self::downloader::download_model_if_needed;
use self::local_stt::LocalStt;
use crate::config::AppConfig;
use anyhow::Result;
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    CreateTranscriptionRequestArgs,
};
use async_openai::Client;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

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
                error!(
                    "[AI] Failed to download/check model: {}. Falling back to API mode.",
                    e
                );
                None
            } else {
                match LocalStt::new(&config.stt_model_path) {
                    Ok(stt) => Some(std::sync::Arc::new(stt)),
                    Err(e) => {
                        error!(
                            "[AI] Failed to initialize local STT: {}. Falling back to API mode.",
                            e
                        );
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
    pub fn notify_wakeup(&self) {
        let mut last_wake = self.last_wake_time.lock().unwrap();
        *last_wake = Some(std::time::Instant::now());
        info!("\x1b[32;1m[WAKE] Hardware-triggered wakeup authorized.\x1b[0m");
    }

    async fn send_local_done_cue(net: &std::sync::Arc<Mutex<OwnedWriteHalf>>) {
        let pkt = [
            crate::protocol::MAGIC_HEADER,
            crate::protocol::MSG_FEEDBACK,
            0,
            0,
        ];
        let mut w = net.lock().await;
        if let Err(e) = w.write_all(&pkt).await {
            warn!("[NET] MSG_FEEDBACK (local done cue) failed: {}", e);
        }
    }

    pub async fn process_utterance(
        &self,
        wav_path: String,
        net: std::sync::Arc<Mutex<OwnedWriteHalf>>,
    ) -> Result<()> {
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
        // 唤醒词检测：两种策略并行
        // 策略 A：精确关键词
        let exact_keywords = ["知行社", "cyberhub", "你好", "您好"];
        // 策略 B：动态音近匹配 — "小" + 任意 zhì/zhī 音汉字
        // 覆盖 Whisper 对"小智"的所有常见同音误认（无需逐一枚举）
        const XIAO_ZHI_CHARS: &str = "智知志字治至支致姿直制质纸值止之只指脂植殖炙挚置帜稚滞";

        let mut wake_word_found = false;
        let mut command = String::new();

        // 策略 A 检查
        for kw in exact_keywords {
            if let Some(pos) = text.to_lowercase().find(kw) {
                wake_word_found = true;
                let raw_cmd = &text[pos + kw.len()..];
                command = raw_cmd
                    .trim_start_matches(|c: char| ",，。 ".contains(c))
                    .to_string();
                break;
            }
        }

        // 策略 B 检查（若策略 A 未命中）
        if !wake_word_found {
            let chars: Vec<char> = text.chars().collect();
            for i in 0..chars.len().saturating_sub(1) {
                if chars[i] == '小' && XIAO_ZHI_CHARS.contains(chars[i + 1]) {
                    wake_word_found = true;
                    // 提取唤醒词之后的指令部分
                    let byte_pos: usize = text
                        .char_indices()
                        .nth(i + 2)
                        .map(|(b, _)| b)
                        .unwrap_or(text.len());
                    command = text[byte_pos..]
                        .trim_start_matches(|c: char| ",，。 ".contains(c))
                        .to_string();
                    break;
                }
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

        info!(
            "\x1b[32;1m[WAKE] Command accepted: \"{}\"\x1b[0m",
            final_command
        );

        // 4. 优先尝试本地指令路由（无网络延迟）
        if Self::try_local_command(&final_command) {
            Self::send_local_done_cue(&net).await;
            return Ok(());
        }

        // 5. 本地无匹配 → 转发 ZeroClaw
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
                Self::send_local_done_cue(&net).await;
            }
            Err(_) => {
                info!("[GATEKEEPER] ZeroClaw relay skipped (Is it running on port 42617?)");
            }
        }

        Ok(())
    }

    /// 本地指令路由器。
    /// 如果识别到已知的本地指令，立即执行并返回 true；否则返回 false（交给 ZeroClaw）。
    fn try_local_command(command: &str) -> bool {
        use crate::system::control;

        // ── 锁屏 ──────────────────────────────────────────────────────────────
        if command.contains("锁屏")
            || command.contains("锁定屏幕")
            || command.contains("lock screen")
        {
            info!("[LOCAL] → lock_screen");
            control::trigger_macos_lock();
            return true;
        }

        // ── 静音 / 取消静音（优先检查，防止被音量逻辑截断）────────────────────
        if command.contains("取消静音")
            || command.contains("解除静音")
            || command.contains("打开声音")
            || command.contains("恢复声音")
        {
            info!("[LOCAL] → unmute");
            control::unmute();
            return true;
        }
        if command.contains("静音") || command.contains("关掉声音") || command.contains("关闭声音")
        {
            info!("[LOCAL] → mute");
            control::mute();
            return true;
        }

        // ── 音量控制（语义分解：主体词 + 方向词，处理"音量再调小一点"等自然说法）──
        let has_volume_subject = command.contains("音量") || command.contains("声音");
        if has_volume_subject {
            let up_words = ["大", "高", "加", "升", "响"];
            let down_words = ["小", "低", "减", "降", "轻"];
            if up_words.iter().any(|w| command.contains(w)) {
                info!("[LOCAL] → volume_up");
                control::volume_up();
                return true;
            }
            if down_words.iter().any(|w| command.contains(w)) {
                info!("[LOCAL] → volume_down");
                control::volume_down();
                return true;
            }
        }

        // 未匹配任何本地指令 → 交给 ZeroClaw
        false
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
