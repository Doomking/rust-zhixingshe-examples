# hub-server — Mac 端中枢（PRD Phase 3.2）

对应《剑匣 CyberHub》白皮书 **§3.2 上位机端**：在本机运行 **TCP 网关**，接收 ESP32-S3 BOX-3 的帧式数据流，将语音交给 STT，再把**文本**意图交给本机 **[ZeroClaw](https://www.zeroclawlabs.ai/)**（[官方文档](https://www.zeroclawlabs.ai/docs) · [中文 README](https://github.com/zeroclaw-labs/zeroclaw/blob/master/docs/i18n/zh-CN/README.md)）。

**产品形态（与文档一致）：** ZeroClaw 是跑在**你自己机器上**的**个人助理运行时**（Gateway 控制面：**HTTP / WebSocket / Webhook**、仪表盘、配对、cron 等；**不是**「托管在云端的 OpenAI 替代品」）。文档里也写明了 **OpenAI-*compatible* provider**（对接各类模型后端）与 **可插拔端点**——那是**出站调用模型**时的能力，**不**等于「ZeroClaw 一定对外长得像公共云 OpenAI 接入点」。

**本仓库当前实现：** 默认 **`ZEROCLAW_MODE=ws_chat`**，连接 Gateway **`GET /ws/chat`** WebSocket（上游 `turn_streamed`，**带工具**）；**`webhook`** 走 `POST /webhook`（上游 **`run_gateway_chat_simple`，无工具**，仅适合闲聊/探测）；**`openai`** 走 `async-openai` + **`ZEROCLAW_URL`**（仅当 Gateway 真的挂了 `/v1`）。协议注释见 [zeroclaw-gateway `ws.rs`](https://github.com/zeroclaw-labs/zeroclaw/blob/master/crates/zeroclaw-gateway/src/ws.rs)。**STT（Whisper）仍在 `hub-server` 内完成**，只把**文字**交给 ZeroClaw。

## 架构（简要）

```
BOX-3  TCP :8080 (可配)          hub-server                 ZeroClaw Gateway :42617
  |  0x5A 帧协议  ──────────►  gateway (解包)            Webhook / WS / REST …
  |                              ├─ AudioProcessor → WAV   （可选：若启用则还有 /v1 Chat）
  |                              ├─ AiProcessor → Whisper(本地/API)
  |  ◄── MSG_METRICS / MSG_FEEDBACK ──  └─ 唤醒词门禁 + 本地指令 / Agent 转发
```

协议常量见 [`src/protocol.rs`](src/protocol.rs)。

## 前置条件

1. **ZeroClaw**：按 [文档 Quick Start](https://www.zeroclawlabs.ai/docs) 使用 **`zeroclaw daemon`**（或 `service`）在本机拉起 **Gateway**（`127.0.0.1:42617` 为默认）。与 BOX-3 对接时，**优先以文档列出的控制面为准**（如 **`POST /webhook`**、**`GET /ws/chat`**）；**`/v1/chat/completions` 仅为部分配置/版本下可能启用的可选入口**，不是「个人助理 = OpenAI 云 API」那种产品定义。仅 `zeroclaw gateway`、或未配对/未启用对应路由时，`hub-server` 里现有的 `/v1` 客户端可能失败；此时本地指令仍可用。
2. **STT**：默认 `USE_INTERNAL_STT=true` 使用本地 `whisper-rs`（需 `STT_MODEL_PATH` 指向 `ggml-*.bin`）。设为 `false` 时走 `AI_BASE_URL` 上的 Whisper 兼容接口。
3. 设备端 `.env` 中 `SERVER_IP` 指向运行 `hub-server` 的 Mac 局域网 IP。

### ZeroClaw 联调：仅 Gateway、能看到 Web 页，但 `hub-server` / `curl` 连不上 `/v1`

**现象 A —** `curl http://127.0.0.1:42617/v1/models` 返回 **整页 HTML**（`<title>ZeroClaw</title>`、`_app/assets/index-*.js`）：说明 **`/v1/models` 没有按 OpenAI API 返回 JSON**，而是落到了 **前端单页应用的 fallback**，OpenAI 兼容路由此时**未挂在你访问的这个端口/进程上**。

**现象 B —** `POST .../v1/chat/completions` 返回 **`405 Method Not Allowed`** 且 **`Allow: GET, HEAD`**：同一类问题——该路径上只有「静态/仪表盘」允许的 GET，没有 Chat Completions 的 POST。

官方 [Operations Runbook](https://github.com/zeroclaw-labs/zeroclaw/blob/master/docs/ops/operations-runbook.md) 里写明：**`zeroclaw gateway`** 侧重 **gateway only / webhook 调试**；**完整前台运行时**用 **`zeroclaw daemon`**，长期驻留可用 **`zeroclaw service install && zeroclaw service start`**。要让 `hub-server` 走 `ZEROCLAW_URL`，一般需要 **daemon（或服务）把完整 Gateway 能力拉起来**，而不是只开一个「只有 Web 壳」的进程。

建议逐项确认：

1. 在本机改为 **`zeroclaw daemon`**（或 **`zeroclaw service start`**）后，再执行：  
   `curl -sS http://127.0.0.1:42617/v1/models` —— 应得到 **JSON**（至少像 OpenAI `List models`），而不是 HTML。
2. **`path_prefix`**：若 `~/.zeroclaw/config.toml` 里 `[gateway]` 设置了 `path_prefix`（例如 `"/zeroclaw"`），则 Base 为 `http://127.0.0.1:42617/zeroclaw/v1`，`ZEROCLAW_URL` 须一致。
3. **端口与进程**：`lsof -i :42617` 或 `zeroclaw status`，确认 42617 上是你期望的 ZeroClaw 运行时。
4. 仍异常时用 **`zeroclaw doctor`**，并对照当前版本的 ZeroClaw 文档，确认 OpenAI HTTP 表面是否在其它端口或需额外配置。

### `zeroclaw daemon` 已启动：横幅里说明了什么

典型日志会写：

- **`POST /webhook`**、`**GET /api/*`**、`**GET /ws/chat`**：这是当前进程**明确挂载**的 HTTP 能力。
- **`Pairing: ACTIVE (bearer token required)`**：除 `/pair` 等公开配对外，带鉴权的 API 需要 **Bearer**。请在 `hub-server` 的 `.env` 里设置 **`ZEROCLAW_API_KEY`**（与写死在代码里的字面量 `zeroclaw` 无关，应为你配对/配置里真实的 Gateway token）。获取方式以你本机 ZeroClaw 版本为准，例如日志提示的 **`zeroclaw gateway get-paircode --new`** 完成配对后，把发给客户端的 token 配进 `hub-server`。
- **`/v1/chat/completions` 若未出现在横幅中**：说明 **OpenAI 形 `/v1` 可能未启用**；请用默认 **`ZEROCLAW_MODE=ws_chat`**（**`/ws/chat`**，带工具）或 **`webhook`**（无工具），勿强依赖 `/v1`。

## 配置

复制 `.env.example` 为 `.env` 并按环境修改。主要变量：

| 变量 | 含义 |
|------|------|
| `SERVER_PORT` | TCP 监听端口（与设备连接 `IP:PORT` 一致） |
| `ZEROCLAW_MODE` | **`auto`**（默认，自动按 `ws_chat -> webhook -> openai` 回退）· `ws_chat` · `webhook` · `openai` |
| `ZEROCLAW_WS_CHAT_URL` | WebSocket URL **不含 query**（默认 `ws://127.0.0.1:42617/ws/chat`）；`token` / `session_id` 由程序追加 |
| `ZEROCLAW_WS_SESSION_ID` | 默认 `cyber-hub`；设为空字符串则每次连接新会话（无跨轮 Gateway 记忆） |
| `ZEROCLAW_WS_SESSION_NAME` | 可选；对应 WS 查询参数 `name` |
| `ZEROCLAW_WEBHOOK_URL` | **`webhook` 模式**：完整 `POST` URL（`path_prefix` 须写进路径） |
| `ZEROCLAW_WEBHOOK_SECRET` | 可选；与 ZeroClaw `channels_config.webhook.secret` 对应 |
| `ZEROCLAW_URL` | 仅 **`openai`**：`async-openai` 的 Base |
| `ZEROCLAW_API_KEY` | Gateway **`Authorization: Bearer`**（`POST /pair` 配对后得到的 token；默认占位 `zeroclaw` 通常无效） |
| `ENABLE_TTS_FEEDBACK` | `true` 时将 ZeroClaw 文本回复在 Mac 端转为 PCM，并通过 `MSG_FEEDBACK` payload 下发到设备播放 |
| `TTS_BACKEND` | `auto`（默认，按系统与可执行自动选）/ `mac_say` / `piper` / `none` |
| `TTS_VOICE` / `TTS_RATE` / `TTS_MAX_CHARS` | `mac_say` 参数（语音、语速、最大朗读字符数） |
| `TTS_PIPER_CMD` / `TTS_PIPER_MODEL` | `piper` 参数（可执行路径、模型 `.onnx` 路径） |
| `AI_BASE_URL` / `AI_API_KEY` / `AI_MODEL` | 对外 STT 或备用 LLM（视 `USE_INTERNAL_STT`） |
| `USE_INTERNAL_STT` | `true`：本地 Whisper；`false`：API 转写 |
| `STT_MODEL_PATH` | 本地 Whisper 模型路径 |
| `AUDIO_STORAGE_PATH` | 会话 PCM 落盘目录 |

## 运行

```bash
cd hub-server
cp .env.example .env   # 首次
cargo run --release
```

## 与 PRD 任务对照

| PRD | 状态 |
|-----|------|
| 3.2 部署 ZeroClaw + 中间层收音频流 | **网关**：`src/gateway/mod.rs`；**ZC**：`src/ai/mod.rs`（`ws_chat` / `webhook` / `openai`） |
| 3.3 Whisper + Agent 逻辑 | `AiProcessor::process_utterance` |
| 3.4 TTS 回传 | 待与设备下行音频协议扩展（当前可用 `MSG_FEEDBACK` 触发设备本地 PCM） |

## 调试：PCM 转 WAV

```bash
ffmpeg -f s16le -ar 16000 -ac 1 -i audio_<session>.pcm out.wav
```

（若设备送立体声，将 `-ac 1` 改为 `-ac 2`。）
