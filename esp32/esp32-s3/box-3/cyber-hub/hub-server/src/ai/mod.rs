pub mod downloader;
pub mod local_stt;

use self::downloader::download_model_if_needed;
use self::local_stt::LocalStt;
use crate::config::{AppConfig, ZcRelayMode};
use anyhow::{Context, Result};
use async_openai::config::{Config, OpenAIConfig};
use async_openai::types::{
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    CreateTranscriptionRequestArgs,
};
use async_openai::Client;
use futures_util::{SinkExt, StreamExt};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub struct AiProcessor {
    stt_client: Client<OpenAIConfig>,
    zc_client: Client<OpenAIConfig>,
    model: String,
    local_stt: Option<std::sync::Arc<LocalStt>>,
    use_internal: bool,
    last_wake_time: std::sync::Mutex<Option<std::time::Instant>>,
    zc_relay_mode: ZcRelayMode,
    zc_webhook_url: String,
    zc_webhook_secret: Option<String>,
    zc_api_key: String,
    zc_ws_chat_url: String,
    zc_ws_session_id: Option<String>,
    zc_ws_session_name: Option<String>,
    enable_tts_feedback: bool,
    tts_voice: String,
    tts_rate: u32,
    tts_max_chars: usize,
    http: reqwest::Client,
}

impl AiProcessor {
    pub async fn new(config: &AppConfig) -> Self {
        let stt_config = OpenAIConfig::new()
            .with_api_base(&config.ai_base_url)
            .with_api_key(&config.ai_api_key);
        let stt_client = Client::with_config(stt_config);

        let zc_config = OpenAIConfig::new()
            .with_api_base(&config.zc_url)
            .with_api_key(&config.zc_api_key);
        let zc_client = Client::with_config(zc_config);

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client");

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
            zc_relay_mode: config.zc_relay_mode,
            zc_webhook_url: config.zc_webhook_url.clone(),
            zc_webhook_secret: config.zc_webhook_secret.clone(),
            zc_api_key: config.zc_api_key.clone(),
            zc_ws_chat_url: config.zc_ws_chat_url.clone(),
            zc_ws_session_id: config.zc_ws_session_id.clone(),
            zc_ws_session_name: config.zc_ws_session_name.clone(),
            enable_tts_feedback: config.enable_tts_feedback,
            tts_voice: config.tts_voice.clone(),
            tts_rate: config.tts_rate,
            tts_max_chars: config.tts_max_chars,
            http,
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

    async fn send_feedback_pcm_chunks(
        net: &std::sync::Arc<Mutex<OwnedWriteHalf>>,
        mono_s16le: &[u8],
    ) -> Result<()> {
        const CHUNK: usize = 1024;
        if mono_s16le.is_empty() {
            Self::send_local_done_cue(net).await;
            return Ok(());
        }
        let mut w = net.lock().await;
        for chunk in mono_s16le.chunks(CHUNK) {
            let len = (chunk.len() as u16).to_le_bytes();
            let header = [
                crate::protocol::MAGIC_HEADER,
                crate::protocol::MSG_FEEDBACK,
                len[0],
                len[1],
            ];
            w.write_all(&header).await?;
            w.write_all(chunk).await?;
        }
        Ok(())
    }

    async fn speak_reply_and_feedback(
        &self,
        reply_text: &str,
        net: &std::sync::Arc<Mutex<OwnedWriteHalf>>,
    ) -> Result<()> {
        let trimmed = reply_text.trim();
        if trimmed.is_empty() {
            Self::send_local_done_cue(net).await;
            return Ok(());
        }
        if !self.enable_tts_feedback {
            Self::send_local_done_cue(net).await;
            return Ok(());
        }
        if let Some(pcm) = self.synthesize_tts_pcm(trimmed).await {
            Self::send_feedback_pcm_chunks(net, &pcm).await?;
        } else {
            Self::send_local_done_cue(net).await;
        }
        Ok(())
    }

    async fn synthesize_tts_pcm(&self, text: &str) -> Option<Vec<u8>> {
        let mut t = text.replace('\n', " ");
        if t.chars().count() > self.tts_max_chars {
            t = t.chars().take(self.tts_max_chars).collect();
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis();
        let aiff = format!("/tmp/cyberhub_tts_{ts}.aiff");
        let pcm = format!("/tmp/cyberhub_tts_{ts}.pcm");

        let say = Command::new("say")
            .arg("-v")
            .arg(&self.tts_voice)
            .arg("-r")
            .arg(self.tts_rate.to_string())
            .arg("-o")
            .arg(&aiff)
            .arg(&t)
            .output()
            .await;
        match say {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                warn!("[TTS] `say` failed: {}", String::from_utf8_lossy(&out.stderr));
                return None;
            }
            Err(e) => {
                warn!("[TTS] spawn `say` failed: {e}");
                return None;
            }
        }

        let ff = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(&aiff)
            .arg("-f")
            .arg("s16le")
            .arg("-ac")
            .arg("1")
            .arg("-ar")
            .arg("16000")
            .arg(&pcm)
            .output()
            .await;
        match ff {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                warn!(
                    "[TTS] `ffmpeg` convert failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let _ = fs::remove_file(&aiff).await;
                return None;
            }
            Err(e) => {
                warn!("[TTS] spawn `ffmpeg` failed: {e}");
                let _ = fs::remove_file(&aiff).await;
                return None;
            }
        }

        let bytes = match fs::read(&pcm).await {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                warn!("[TTS] generated empty PCM");
                let _ = fs::remove_file(&aiff).await;
                let _ = fs::remove_file(&pcm).await;
                return None;
            }
            Err(e) => {
                warn!("[TTS] read PCM failed: {e}");
                let _ = fs::remove_file(&aiff).await;
                let _ = fs::remove_file(&pcm).await;
                return None;
            }
        };

        let _ = fs::remove_file(&aiff).await;
        let _ = fs::remove_file(&pcm).await;
        Some(bytes)
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

        // 5. 本地无匹配 → 转发 ZeroClaw（WS 带工具 / Webhook 无工具 / OpenAI 形 /v1）
        match self.zc_relay_mode {
            ZcRelayMode::WsChat => {
                self.relay_zeroclaw_ws_chat(&final_command, &net).await?;
            }
            ZcRelayMode::Webhook => {
                self.relay_zeroclaw_webhook(&final_command, &net).await?;
            }
            ZcRelayMode::Openai => {
                let zc_request = CreateChatCompletionRequestArgs::default()
                    .model(&self.model)
                    .messages([ChatCompletionRequestUserMessageArgs::default()
                        .content(final_command)
                        .build()?
                        .into()])
                    .build()?;

                debug!(
                    target: "hub_server::zeroclaw",
                    url = %self.zc_client.config().api_base(),
                    payload = %serde_json::to_string(&zc_request).unwrap_or_else(|e| e.to_string()),
                    "ZeroClaw chat request (OpenAI-compatible JSON)"
                );

                match self.zc_client.chat().create(zc_request).await {
                    Ok(zc_response) => {
                        debug!(
                            target: "hub_server::zeroclaw",
                            body = %serde_json::to_string(&zc_response).unwrap_or_else(|e| e.to_string()),
                            "ZeroClaw chat response (parsed OK)"
                        );
                        let mut reply_text: Option<String> = None;
                        if let Some(choice) = zc_response.choices.first() {
                            if let Some(content) = &choice.message.content {
                                println!("\x1b[36m[ZeroClaw] Agent Response: \"{}\"\x1b[0m", content);
                                reply_text = Some(content.clone());
                            }
                        }
                        if let Some(reply) = reply_text {
                            self.speak_reply_and_feedback(&reply, &net).await?;
                        } else {
                            Self::send_local_done_cue(&net).await;
                        }
                    }
                    Err(e) => {
                        error!("[ZC] ZeroClaw chat error: {e:#}");
                        info!(
                            "[GATEKEEPER] ZeroClaw relay skipped. \
                             If deserialization failed, scroll for `failed deserialization of:` (raw body). \
                             Enable RUST_LOG=hub_server::ai=debug,hub_server::zeroclaw=debug for request JSON."
                        );
                    }
                }
            }
        }

        Ok(())
    }

    fn build_zeroclaw_ws_url(&self) -> Result<String> {
        let mut u = url::Url::parse(self.zc_ws_chat_url.trim())
            .context("ZEROCLAW_WS_CHAT_URL must be a valid URL (e.g. ws://127.0.0.1:42617/ws/chat)")?;
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("token", self.zc_api_key.trim());
            if let Some(ref sid) = self.zc_ws_session_id {
                if !sid.trim().is_empty() {
                    q.append_pair("session_id", sid.trim());
                }
            }
            if let Some(ref name) = self.zc_ws_session_name {
                if !name.trim().is_empty() {
                    q.append_pair("name", name.trim());
                }
            }
        }
        Ok(u.to_string())
    }

    /// `GET /ws/chat` — ZeroClaw Gateway WebSocket（`process_chat_message` / `turn_streamed`，**含工具**）。
    /// 协议见 upstream `crates/zeroclaw-gateway/src/ws.rs` 文件头注释。
    async fn relay_zeroclaw_ws_chat(
        &self,
        message: &str,
        net: &std::sync::Arc<Mutex<OwnedWriteHalf>>,
    ) -> Result<()> {
        let url = match self.build_zeroclaw_ws_url() {
            Ok(u) => u,
            Err(e) => {
                error!("[ZC] ws: {:#}", e);
                return Ok(());
            }
        };

        debug!(
            target: "hub_server::zeroclaw",
            url = %url,
            "ZeroClaw WebSocket connect"
        );

        let (ws, _) = match timeout(Duration::from_secs(20), connect_async(&url)).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                error!("[ZC] ws handshake failed: {e:#}");
                info!("[GATEKEEPER] ZeroClaw ws relay skipped.");
                return Ok(());
            }
            Err(_) => {
                error!("[ZC] ws connect timed out (20s)");
                info!("[GATEKEEPER] ZeroClaw ws relay skipped.");
                return Ok(());
            }
        };

        let (mut write, mut read) = ws.split();

        // Server normally sends `session_start` first; tolerate leading Ping frames.
        let mut got_server_text = false;
        for _ in 0..8u8 {
            match timeout(Duration::from_secs(10), read.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => {
                    debug!(target: "hub_server::zeroclaw", first = %t, "ZeroClaw ws server text frame");
                    got_server_text = true;
                    break;
                }
                Ok(Some(Ok(Message::Ping(p)))) => {
                    let _ = write.send(Message::Pong(p)).await;
                }
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                    warn!("[ZC] ws closed before session_start");
                    return Ok(());
                }
                Ok(Some(Err(e))) => {
                    error!("[ZC] ws read: {e:#}");
                    return Ok(());
                }
                Ok(Some(Ok(_))) => continue,
                Err(_) => {
                    warn!("[ZC] ws timeout waiting for first server text frame");
                    return Ok(());
                }
            }
        }
        if !got_server_text {
            warn!("[ZC] ws no server text frame after initial reads");
            return Ok(());
        }

        let outgoing = serde_json::json!({ "type": "message", "content": message }).to_string();
        if let Err(e) = write.send(Message::Text(outgoing.into())).await {
            error!("[ZC] ws send message: {e:#}");
            return Ok(());
        }

        let turn = async {
            let mut success = false;
            let mut reply_text: Option<String> = None;
            loop {
                let raw = match read.next().await {
                    None => {
                        warn!("[ZC] ws stream ended without `done`");
                        break;
                    }
                    Some(Err(e)) => {
                        error!("[ZC] ws read: {e:#}");
                        break;
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = write.send(Message::Pong(p)).await;
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => continue,
                    Some(Ok(Message::Close(_))) => {
                        warn!("[ZC] ws server closed connection");
                        break;
                    }
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Binary(_))) => continue,
                    Some(Ok(other)) => {
                        debug!(target: "hub_server::zeroclaw", ?other, "ZeroClaw ws skipped frame");
                        continue;
                    }
                };

                let v: serde_json::Value = match serde_json::from_str(raw.as_str()) {
                    Ok(v) => v,
                    Err(e) => {
                        debug!(target: "hub_server::zeroclaw", err=%e, frame=%raw, "ZeroClaw ws non-JSON frame");
                        continue;
                    }
                };

                let ty = v.get("type").and_then(|x| x.as_str());
                match ty {
                    Some("done") => {
                        let full = v
                            .get("full_response")
                            .and_then(|x| x.as_str())
                            .unwrap_or("");
                        if full.is_empty() {
                            warn!("[ZC] ws `done` with empty full_response");
                        } else {
                            println!("\x1b[36m[ZeroClaw] \"{}\"\x1b[0m", full);
                            reply_text = Some(full.to_string());
                        }
                        success = true;
                        break;
                    }
                    Some("error") => {
                        let msg = v
                            .get("message")
                            .and_then(|x| x.as_str())
                            .unwrap_or("unknown error");
                        let code = v.get("code").and_then(|x| x.as_str()).unwrap_or("");
                        error!("[ZC] ws agent error ({code}): {msg}");
                        break;
                    }
                    Some("tool_call") => {
                        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        info!(
                            "\x1b[33m[ZeroClaw/tool]\x1b[0m call `{}` args {}",
                            name,
                            v.get("args").map(|a| a.to_string()).unwrap_or_default()
                        );
                    }
                    Some("tool_result") => {
                        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        let out = v
                            .get("output")
                            .map(|o| o.to_string())
                            .unwrap_or_default();
                        let preview: String = out.chars().take(400).collect();
                        let ellipsis = if out.chars().count() > 400 { "…" } else { "" };
                        info!(
                            "\x1b[33m[ZeroClaw/tool]\x1b[0m result `{}`: {}{}",
                            name, preview, ellipsis
                        );
                    }
                    Some("chunk") | Some("thinking") => {
                        debug!(
                            target: "hub_server::zeroclaw",
                            ty = ?ty,
                            content = %v.get("content").and_then(|x| x.as_str()).unwrap_or(""),
                            "ZeroClaw ws stream chunk"
                        );
                    }
                    Some(
                        "session_start" | "connected" | "chunk_reset" | "agent_start" | "agent_end",
                    ) => {}
                    _ => {
                        debug!(target: "hub_server::zeroclaw", frame = %v, "ZeroClaw ws other frame");
                    }
                }
            }
            (success, reply_text)
        };

        let (ok, reply_text) = match timeout(Duration::from_secs(300), turn).await {
            Ok(v) => v,
            Err(_) => {
                error!("[ZC] ws agent turn timed out (300s)");
                (false, None)
            }
        };

        if ok {
            if let Some(reply) = reply_text {
                self.speak_reply_and_feedback(&reply, net).await?;
            } else {
                Self::send_local_done_cue(net).await;
            }
        }

        Ok(())
    }

    /// `POST /webhook` — ZeroClaw Gateway（见 upstream `WebhookBody` / `handle_webhook`）。
    async fn relay_zeroclaw_webhook(
        &self,
        message: &str,
        net: &std::sync::Arc<Mutex<OwnedWriteHalf>>,
    ) -> Result<()> {
        let payload = serde_json::json!({ "message": message });
        debug!(
            target: "hub_server::zeroclaw",
            url = %self.zc_webhook_url,
            payload = %payload,
            "ZeroClaw webhook request"
        );

        let mut req = self
            .http
            .post(&self.zc_webhook_url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.zc_api_key.trim()),
            )
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);

        if let Some(ref secret) = self.zc_webhook_secret {
            req = req.header("X-Webhook-Secret", secret);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                error!("[ZC] webhook transport error: {e:#}");
                info!("[GATEKEEPER] ZeroClaw webhook relay skipped.");
                return Ok(());
            }
        };

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        debug!(
            target: "hub_server::zeroclaw",
            status = %status,
            body = %body_text,
            "ZeroClaw webhook HTTP response"
        );

        if !status.is_success() {
            error!("[ZC] webhook HTTP {} — {}", status, body_text);
            info!("[GATEKEEPER] ZeroClaw webhook relay skipped.");
            return Ok(());
        }

        let v: serde_json::Value = match serde_json::from_str(&body_text) {
            Ok(v) => v,
            Err(e) => {
                error!("[ZC] webhook JSON parse error: {e:#} body={body_text:?}");
                return Ok(());
            }
        };

        if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
            error!("[ZC] webhook error field: {}", err);
            info!("[GATEKEEPER] ZeroClaw webhook relay skipped.");
            return Ok(());
        }

        if v.get("status").and_then(|x| x.as_str()) == Some("duplicate") {
            info!("[ZC] webhook idempotent duplicate — no new reply");
            return Ok(());
        }

        let reply = v
            .get("response")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let model_label = v
            .get("model")
            .and_then(|x| x.as_str())
            .unwrap_or("");

        if reply.is_empty() {
            warn!("[ZC] webhook OK but empty `response` field");
        } else if model_label.is_empty() {
            println!("\x1b[36m[ZeroClaw] \"{}\"\x1b[0m", reply);
        } else {
            println!(
                "\x1b[36m[ZeroClaw] ({}) \"{}\"\x1b[0m",
                model_label, reply
            );
        }

        if !reply.is_empty() {
            self.speak_reply_and_feedback(&reply, net).await?;
        } else {
            Self::send_local_done_cue(net).await;
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
