# CyberHub（ESP32-S3-BOX-3 + ZeroClaw）

CyberHub 是一个“设备端 + 服务端 + 本地 Agent”的语音物理助手系统：

- 设备端（`hub-device-std`）采集音频、发送事件、播放下行音频
- 服务端（`hub-server`）做网关、STT、本地控制、Agent 转发
- Agent 端用本机 ZeroClaw（`daemon`）执行工具调用并返回结果

核心目标是：对设备说"Hi ESP"，ZeroClaw 能“理解并执行”，并把结果回传到设备（文本/语音）。

## 1) 功能与技术方案

- 语音主链路：`BOX-3 PCM -> hub-server STT -> ZeroClaw(ws_chat/webhook/openai) -> 回复 -> TTS -> BOX-3 播放`
- 协议：设备与服务端使用自定义 TCP 帧协议（`0x5A` 头）
- Agent 接入：默认 `ZEROCLAW_MODE=auto`，自动按 `ws_chat -> webhook -> openai` 回退
- 工具能力：优先 `ws_chat`（ZeroClaw `turn_streamed`，带工具）
- 跨平台策略：
  - TTS：`TTS_BACKEND=auto`（自动选 `mac_say` / `piper` / `none`）
  - 本地系统控制（锁屏/音量）按 OS 自动分发（macOS/Linux/Windows）

## 2) 启动流程（端到端）

### 2.1 配置环境变量

#### 设备端（`hub-device-std`）

```bash
cd hub-device-std
cp .env.example .env
```

至少修改：

- `WIFI_SSID`
- `WIFI_PASS`
- `SERVER_IP`（运行 `hub-server` 的机器 IP）

#### 服务端（`hub-server`）

```bash
cd hub-server
cp .env.example .env
```

至少修改：

- `ZEROCLAW_API_KEY`（配对后 token，见下文）
- 如有需要：`ZEROCLAW_WS_CHAT_URL`、`TTS_BACKEND`、`TTS_PIPER_MODEL`

### 2.2 先启动依赖：ZeroClaw

```bash
zeroclaw daemon
```

### 2.3 再启动服务端

```bash
cd hub-server
cargo run --release
```

### 2.4 最后启动设备端

```bash
cd hub-device-std
# 常见方式：按项目 runner 流程刷写并监控
cargo run --release
```

> `runner.sh` 会先写 `srmodels.bin`，再 flash 并进入 monitor。

## 3) ZeroClaw 依赖与 Token 获取

ZeroClaw 开启 pairing 时，`hub-server` 需要 `Authorization: Bearer <token>`。

### 3.1 获取 pairing code

```bash
zeroclaw gateway get-paircode --new
```

### 3.2 用 pairing code 换 token

```bash
PAIR_CODE="<上一步配对码>"
curl -sS -X POST \
  -H "X-Pairing-Code: ${PAIR_CODE}" \
  "http://127.0.0.1:42617/pair"
```

返回 JSON 里会有 `token`，填入：

- `hub-server/.env` -> `ZEROCLAW_API_KEY=<token>`

## 4) 运行时模式建议

- `ZEROCLAW_MODE=auto`（默认）：自动尝试工具能力最强路径
- `TTS_BACKEND=auto`（默认）：自动匹配当前系统可用工具链
- 仅调试无工具聊天可用 `webhook`；生产建议保留 `auto` 或强制 `ws_chat`

## 5) 常见排查

- `ws_chat` 连不上：
  - 确认 `zeroclaw daemon` 正在运行
  - 检查 `ZEROCLAW_API_KEY` 是否为配对 token
- `/v1` 返回 HTML/405：
  - 正常，说明当前未挂 OpenAI 形路由；`auto` 会回退到其它路径
- 无语音回传：
  - 检查 `ENABLE_TTS_FEEDBACK=true`
  - `TTS_BACKEND=auto` 时确认本机有对应后端（mac: `say`+`ffmpeg`；piper: `piper`+model+`ffmpeg`）

---

更细的服务端说明见 `hub-server/README.md`。
