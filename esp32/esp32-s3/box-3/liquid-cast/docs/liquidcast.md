# 桌面 AI 助手：端云一体化全彩流媒体播放方案 (LiquidCast)

## 1. 工程概述 (Project Overview)
本项目旨在基于 ESP32-S3-Box 3 (STD 环境) 和 Mac 宿主机实现一套高性能、端云一体的全彩音视频流媒体播放器。
宿主机承担所有重负载操作（视频解码、缩放、JPEG 编码、音频重采样），通过 Wi-Fi (TCP) 实时推流。设备端利用双核优势和硬件总线（SPI DMA + I2S DMA）进行极限渲染。

* **架构模式**：Monorepo（单体仓库），包含 `mac_server` 和 `esp_client` 两个子工程。
* **通信协议**：自定义二进制 TCP 长连接流协议。
* **视频策略**：MJPEG (Motion JPEG) 画面流，分辨率 320x240。
* **音频策略**：PCM RAW 流，16kHz, 16-bit, 单声道。

## 2. 目录结构 (Monorepo Structure)
AI 需要按照以下结构初始化 Cargo 工作空间：

```text
liquid_cast_workspace/
├── Cargo.toml                # Workspace 根配置
├── mac_server/               # Mac 端推流服务 (标准 Rust)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs           # 服务入口与 TCP Server
│       ├── media.rs          # 视频解码与 JPEG 压缩
│       └── protocol.rs       # 封包协议实现
└── esp_client/               # ESP32-S3 设备端 (esp-idf-hal)
    ├── Cargo.toml
    ├── build.rs              # ESP-IDF 构建脚本
    ├── sdkconfig.defaults    # 开启 PSRAM 和高频 CPU 的配置
    └── src/
        ├── main.rs           # 硬件初始化与主循环
        ├── network.rs        # Wi-Fi 与 TCP Client
        ├── decoder.rs        # JPEG 解码器 (封装 esp_jpeg)
        ├── render.rs         # SPI 屏幕驱动与 DMA 双缓冲
        └── audio.rs          # I2S 音频输出与 RingBuffer
```

## 3. 自定义流媒体协议 (Liquid Stream Protocol)
为了保证解析速度，使用极简的定长 Header + 变长 Payload 设计。AI 需要在两端实现该协议的序列化与反序列化。

**Frame Header (12 Bytes):**
* `Magic Word` (2 Bytes): `0x4C 0x43` ("LC" for LiquidCast)
* `Frame Type` (1 Byte): `0x01` (Video JPEG) | `0x02` (Audio PCM)
* `Timestamp` (4 Bytes): 毫秒级时间戳 (用于音视频同步)
* `Payload Length` (4 Bytes): 后续数据的字节数
* `Checksum` (1 Byte): Header 自身的简单 XOR 校验和

## 4. 子工程规格说明 (Sub-project Specifications)

### 4.1 `mac_server` (Mac 推流端)
* **核心依赖**：
    * `tokio`: 异步 TCP Server。
    * `video-rs` 或 `ffmpeg-next`: 读取 MP4 视频并提取原始帧与音频。
    * `turbojpeg` 或 `image`: 将原始 RGB 帧极致压缩为 JPEG (质量设为 60-70%，目标体积 < 20KB/帧)。
    * `rubato`: 音频重采样（转为 16kHz）。
* **核心逻辑要求**：
    1.  监听 TCP 端口（如 `0.0.0.0:8080`）。
    2.  接受 ESP32 的连接后，开启读取本地 MP4 文件的 Pipeline。
    3.  **视频流**：缩放为 320x240 -> 压缩为 JPEG -> 封装 Video Frame -> 压入发送队列。
    4.  **音频流**：重采样 -> 切割为固定大小的 Chunk (如 2048 Bytes) -> 封装 Audio Frame -> 压入发送队列。
    5.  严格按照视频的 FPS (如 20fps) 控制发送速率，避免撑爆 ESP32 的 TCP 接收窗口。

### 4.2 `esp_client` (ESP32-S3 设备端)
* **环境与依赖**：
    * 基于 `esp-idf-hal` (STD 环境)。
    * 必须在 `sdkconfig.defaults` 中开启：`CONFIG_ESP32S3_SPIRAM_SUPPORT=y` (Octal PSRAM), `CONFIG_FREERTOS_HZ=1000`, `CONFIG_COMPILER_OPTIMIZATION_PERF=y`。
    * `esp-idf-sys`: 用于 FFI 调用乐鑫官方的 `esp_jpeg` 解码库。
    * `mipidsi` / `display-interface-spi`: 屏幕驱动。
* **核心逻辑要求 (并发架构)**：
    * **线程 1 (Network)**: 阻塞读取 TCP 流，解析 Header。若是 Audio 数据，直接 Push 到音频 RingBuffer；若是 Video 数据，交出所有权给解码线程。
    * **线程 2 (Audio Master)**: 从 RingBuffer 持续拉取 PCM 数据，通过 `esp_idf_hal::i2s::I2sDriver` 写入 DMA。维护并暴露一个全局原子变量 `CURRENT_AUDIO_TIME` 作为系统时钟。
    * **线程 3 (Video Decode & Render)**:
        1. 接收到 JPEG 字节。
        2. 调用 C 接口 `esp_jpeg_decode` 将其解压为 RGB565。
        3. **音画同步校验**：对比该视频帧的 Timestamp 与 `CURRENT_AUDIO_TIME`。若视频落后 > 100ms，则丢弃当前帧 (Drop)；若超前，则 `thread::sleep` 等待。
        4. 获取空闲的显存 Buffer，写入 RGB565 数据。
        5. 触发 SPI DMA 异步传输刷新屏幕。

## 5. AI 执行步骤指南 (AI Execution Plan)

* **Phase 1: 基础设施搭建与网络打通**
    * 初始化 Workspace，建立双端空工程。
    * 编写网络协议的序列化与反序列化代码 (Protocol Layer)。
    * 实现双端 TCP 连接。Mac 端发送 Dummy Data（假数据），ESP32 端接收并打印速率统计。
    * *验收标准*：ESP32 能够稳定接收 TCP 数据且不 OOM。
* **Phase 2: Mac 端媒体处理管道**
    * 引入视频解析库，成功读取 MP4。
    * 实现逐帧提取 -> 缩放 320x240 -> JPEG 压缩。保存几张生成的 JPEG 到本地进行肉眼验证。
    * 实现音频重采样管道，导出测试 `.wav` 文件验证音质。
* **Phase 3: ESP32 端音频播放**
    * 配置 I2S 驱动，连接 BOX-3 的功放与扬声器。
    * 联调：Mac 端发送封装好的真实 Audio Frame，ESP32 接收并播放。
    * *验收标准*：听到连续无破音的音频。
* **Phase 4: ESP32 端视频解码与双缓冲刷屏**
    * 配置 SPI 屏幕驱动与 DMA 缓冲区。
    * 引入 `esp_jpeg` 进行硬件加速解码。
    * 联调：Mac 端发送纯 Video Frame。
    * *验收标准*：屏幕流畅播放无声视频。
* **Phase 5: 终极拼图 - 音画同步 (A/V Sync)**
    * 引入音频时钟基准和视频丢帧补偿逻辑。
    * 全链路联调。

## 6. 避坑指南 (Critical Debugging Hints for AI)
1.  **PSRAM 踩坑**：RGB565 双缓冲（约 300KB）必须通过 `Box::new` 或自定义 Allocator 分配在 PSRAM 中，绝对不能放在栈上，否则立刻 Stack Overflow。
2.  **SPI DMA 限制**：ESP32 的单次 SPI DMA 传输字节数有上限（通常是 4092 字节或 32768 字节），刷全屏时需要将其切分为多个 DMA Chunk 发送，`mipidsi` 或底层 HAL通常有处理，但需重点关注。
3.  **JPEG 解码性能**：切勿在 ESP32 上尝试软解 H.264。如果纯 Rust 的 `jpeg-decoder` 帧率低于 15fps，必须切换到 `esp-idf-sys` 的 C 绑定调用硬件指令优化的 `esp_jpeg`。

## 7. 架构升级（Blueprint v1 已落地）

当前实现已从“纯媒体流”升级到“控制面 + 媒体面”双平面：

1. **共享协议库**
   - 新增 `liquid-protocol` crate，双端统一引用，避免协议漂移。
   - 帧类型新增：`ControlHello` / `ControlAck` / `ControlPing`。

2. **控制面握手与能力协商**
   - ESP 连接后发送 `HELLO`（协议版本、媒体参数、A/V 阈值建议）。
   - Mac 返回 `ACK`（最终协商参数，支持服务端覆盖）。
   - 向后兼容：旧端可跳过握手继续流媒体。

3. **运行时参数下发**
   - Mac 端周期下发 `ControlAck`（当前配置快照）。
   - ESP 端可在运行中接收并应用 A/V 同步阈值（`drop_late_ms` / `wait_ahead_ms`）。

4. **会话保活与指标**
   - Mac 端周期发送 `ControlPing` 保活。
   - ESP 端统计 `ctrl` 控制帧吞吐。
   - ESP 端每 2 秒输出 A/V 指标：`samples / drops / drop_ratio / avg_delta`。
   - Mac 端每 2 秒输出 session 发送统计：`video / audio / ctrl / bytes`。

5. **Workspace 治理**
   - 项目改为 `Cargo workspace`（`liquid-protocol`, `mac-server`, `esp-client`）。
   - 统一 `resolver` 与 profile 到 workspace 根，减少构建配置漂移。
