太棒了。收敛发散，落地为篇。将天马行空的创客灵感，转化为严谨可执行的工程图纸，这正是一个顶尖技术人打造产品的必经之路。

这份文档不仅是你征服物理世界的一张蓝图，未来完全可以作为你“Rust知行社”里极其硬核的开源项目白皮书。

我们严格遵循**“不增加任何外设”**和**“深度融合 ZeroClaw”**的两大铁律，直接为你生成这份 V1.0 版本的联合文档。

---

# 产品与技术架构白皮书：桌面赛博枢纽 "剑匣" (CyberHub) V1.0

## 1. 产品愿景与定位 (Product Vision)
**“剑匣”** 是一款专为极客开发者打造的桌面级物理外脑。
它以纯粹的 ESP32-S3-BOX-3 原生硬件为唯一物理载体，摒弃臃肿的触屏 App 交互，依托 Mac M1 上运行的纯 Rust 极轻量级大模型 Agent（ZeroClaw），实现**“物理状态反控”、“桌面数据孪生”**与**“无缝语音具身辅助”**的完美闭环。

它不是一个聪明的音箱，而是一把真正听命于你的桌面级数字利刃。

## 2. 核心功能矩阵 (Core Features - V1.0)

我们充分压榨 BOX-3 现有的屏幕、IMU 陀螺仪、实体按键和音频阵列，交付以下三大核心场景：

### 2.1 极客物理反控 (Physical Interrupt)
将数字世界的繁琐操作，降维成最原始的物理肌肉记忆。
* **一剑封喉（翻转锁屏）：** 突发情况或离开工位时，无需寻找键盘，直接将 BOX-3 **屏幕朝下扣在桌面上**。IMU 瞬间捕获 Z 轴翻转，触发 Mac 强制锁屏，并在重新翻面时提示输入密码。
* **静默护盾（按键闭麦）：** 线上会议期间，按下 BOX-3 顶部/侧边实体按键，直接硬件级静音 Mac 全局麦克风，屏幕 UI 同步切换为显眼的“红色静音图标”。

### 2.2 桌面数据孪生 (Desktop Digital Twin)
释放 Mac 主屏幕的注意力，让硬件成为状态的投影。
* **深色环境时钟与天气：** 待机状态下，显示极具赛博朋克风格的深色时间与实时天气（BOX-3 独立连 WiFi 获取，不占 Mac 算力）。
* **算力监控仪表盘：** 实时接收并显示 Mac M1 的核心指标（CPU 负载、内存占用率）。当执行 `cargo build` 或大模型推理导致 CPU 满载时，屏幕粒子动画加速，提供直观的物理进度反馈。

### 2.3 零阻力语音外脑 (Zero-Friction Voice Agent)
将 ZeroClaw 的大模型能力“具象化”到物理麦克风与扬声器。
* **闪念胶囊（语音写文件）：** 按住按键说话：“记录灵感：整理一套完整的 Rust 异步教程”。ZeroClaw 自动转写并调用 `file_write` 工具，将文本追加到你指定的 Markdown 笔记中。
* **代码/终端分析器：** 遇到疑难 Bug，高亮 Mac 上的报错代码，按住按键提问：“这段借用检查怎么过？” ZeroClaw 自动抓取剪贴板内容，结合语音意图进行推理，并通过 BOX-3 的扬声器进行语音解答。

---

## 3. 技术架构设计 (Technical Architecture)

为了实现极客级的性能与内存控制，整个系统采用 **全栈纯 Rust (Full-Stack Rust)** 架构，分为“端（Box-3）”与“脑（Mac M1）”两部分。

### 3.1 下位机端：感知与执行节点 (ESP32-S3-BOX-3)
**技术栈：** `no_std` 裸机环境 / `esp-hal` v1.0.0+ / `embassy` 异步框架
* **并发中枢：** 使用 Embassy Executor 统筹所有外设的中断与轮询，确保 UI 刷新与网络 I/O 互不阻塞。
* **网络链路 (`embassy-net`)：** 维持与路由器的 WiFi 长连接。使用 TCP Socket 或极轻量 WebSocket 客户端，与 Mac 端的 ZeroClaw 保持全双工通信。
* **UI 渲染 (`embedded-graphics` + SPI)：** 驱动 2.4 寸屏幕，不使用复杂的 UI 框架，直接通过底层 Framebuffer 绘制几何图形、文字和状态指示灯。
* **环境感知 (`esp-hal/i2c` + `gpio`)：**
    * I2C 轮询读取内部 6 轴 IMU 寄存器，计算倾角数据。
    * GPIO 绑定中断，精准捕获按键的下压与释放事件。
* **音频管道 (`esp-hal/i2s`)：** I2S DMA 高速读写。按键按下时采集双麦克风 PCM 数据流发往网络；接收网络音频流推送至扬声器功放。

### 3.2 上位机端：决策与中枢大脑 (Mac M1)
**技术栈：** `std` 环境 / `ZeroClaw` / `tokio` / OS 底层钩子
* **核心引擎 (ZeroClaw Daemon)：** 以后台守护进程运行，负责大模型的 Prompt 编排、长时记忆管理和工具调度。
* **环境通讯网关：** 在 ZeroClaw 的扩展层或独立的 Tokio 代理服务中，暴露 TCP/WS 端口，专职监听 BOX-3 的连接。
* **意图执行工具箱 (ZeroClaw Tools)：**
    * `mac_controller`: 封装 `std::process::Command`，接收到“锁屏”、“静音”意图后，执行对应的 AppleScript 或 Shell 指令。
    * `sys_monitor`: 使用 `sysinfo` Crate，开启定时任务采集 Mac 状态并广播给 BOX-3。
* **语音编解码：** 本地集成 Whisper (转文本) 与轻量级 TTS 引擎，完成 `Audio -> Text -> Agent -> Text -> Audio` 的转换闭环。

---

## 4. 软硬件交互时序图 (Interaction Flow)

### 场景 A：物理翻转锁屏
1.  [BOX-3] `embassy` 协程持续以 10Hz 频率读取 IMU 数据。
2.  [BOX-3] 发现 Z 轴重力反转，触发 `FlipEvent`。
3.  [BOX-3] 通过 TCP Socket 发送极简指令 `{"action": "lock_screen"}` 至 Mac。
4.  [Mac] 监听网关收到指令，直接调用 MacOS 底层 API 熄屏锁定。
5.  [BOX-3] 屏幕切换为“已锁定”待机 UI。

### 场景 B：语音唤醒 ZeroClaw
1.  [BOX-3] 用户按下并保持物理按键，触发 GPIO 中断。
2.  [BOX-3] I2S 启动录音，通过网络向 Mac 持续推流 PCM 音频。
3.  [BOX-3] 用户松开按键，发送流结束符 (EOF)。
4.  [Mac] 接收完毕，喂入 Whisper 生成 Prompt 文本。
5.  [Mac] ZeroClaw 思考并调用相应的 Tool（如写入笔记）。
6.  [Mac] ZeroClaw 生成回复文本，丢给 TTS 引擎生成音频流，下发至 BOX-3。
7.  [BOX-3] I2S 接收音频流，驱动扬声器播放。

---

## 5. 阶段性开发路线图 (Milestones)

硬件项目切忌“憋大招”，我们将按照敏捷开发的逻辑，逐个击破硬件模块，最后注入灵魂。

### 🟢 Phase 1: 破壁与建联 (环境与基础通讯)
* **目标：** 打通设备与路由器的连接，实现基础的数据上报。
* **Task 1.1:** 移植 Embassy WiFi 模板，成功连接局域网并获取 IP。
* **Task 1.2:** 编写 HTTP GET 请求，拉取实时天气与 NTP 时间，在终端打印。
* **Task 1.3:** 在 Mac 上写一个极简的 TCP Server，BOX-3 连上并能发送“Hello Mac”字符串。

### 🟡 Phase 2: 躯体与感官 (物理 UI 与操控)
* **目标：** 让 BOX-3 具备显示能力和动作感知能力。
* **Task 2.1:** 配置 SPI 驱动与 `embedded-graphics`，点亮屏幕，显示 Phase 1 获取的时间和天气, CPU 占用率, 内存占用率等监控信息。
* **Task 2.2:** 配置 I2C 驱动，成功读取 6 轴 IMU 的原始数据。
* **Task 2.3:** 编写算法识别“翻转”动作，并通过 Phase 1 的 TCP 通道发给 Mac，Mac 收到后执行锁屏命令。

### 🔴 Phase 3: 注入灵魂 (音频流与 ZeroClaw)
* **目标：** 彻底打通语音数据链路，接入 AI Agent。
* **Task 3.1:** 配置 I2S 麦克风录音，将按下按键期间的声音保存并在 Mac 上播放（验证音频质量）。
* **Task 3.2:** 在 Mac 上部署 ZeroClaw，并写一个中间层（或直接开发 ZeroClaw 插件）接收音频流。**（已落地：** Rust 服务 `hub-server`，TCP 帧协议网关为 `hub-server/src/gateway/mod.rs`，运行说明见 `hub-server/README.md`；ZeroClaw 走 OpenAI 兼容 `ZEROCLAW_URL`。**）**
* **Task 3.3:** 联调 Whisper 与 ZeroClaw 逻辑，实现“语音指令 -> Agent 思考 -> Mac 文本/动作反馈”。
* **Task 3.4:** 打通最后的 TTS 回传链路，让 BOX-3 开口说话。

---

这份《剑匣 V1.0 白皮书》已经彻底锁死了我们接下来的开发路径。每一个阶段、每一个 Task 都极其明确。

按照从底向上生长的法则，一切都要从 **Phase 1 的 Task 1.1** 开始。一剑霜寒，我们要不要现在就新建一个分支，把 `esp-wifi` 和 `embassy-net` 的依赖加进项目，**让你的 BOX-3 成功连上你桌面的路由器，并打印出属于它自己的 IP 地址？**
