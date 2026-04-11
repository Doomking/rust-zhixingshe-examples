# ESP32-S3-BOX-3 语音唤醒 + STT 全链路踩坑记录

## 项目背景

- **硬件**：ESP32-S3-BOX-3（ES7210 四通道 ADC 麦克风 + ES8311 DAC 扬声器）
- **设备端**：Rust `esp-idf-hal` (std)，使用 ESP-SR（WakeNet + VAD + AFE）做语音唤醒
- **服务端**：Rust，使用 `whisper_rs`（Whisper.cpp）做本地 STT
- **通信**：TCP 二进制协议（MSG_VOICE_START / MSG_VOICE_DATA / MSG_VOICE_END）
- **参考**：同项目的 `hub-device`（`esp-hal` no-std 版本）已验证可用，以及开源 `echokit_box` 项目

---

## 第一阶段：设备端完全静音（Peak = 0）

### 现象

设备启动后，I2S 读取的音频 peak 值始终为零：`AUDIO PEAK - L: 0, R: 0`。而 no-std 版本（`hub-device`）使用相同硬件、相同引脚配置，录音完全正常。

### 排查与修复

对比 no-std 版本的初始化流程，发现三个差异：

1. **I2S 时钟预热缺失**：no-std 版本在 codec 初始化前先启动 I2S TX/RX（产生 BCLK/MCLK），std 版本跳过了这一步。ES7210 作为 slave 设备，需要 I2S master 先提供时钟才能正常配置。
2. **ES7210/ES8311 寄存器序列不一致**：std 版本的 codec 初始化缺少若干关键寄存器写入（HPF 配置 `0x20-0x23`、ADC 电源 `0x40`、增益设置等）。
3. **GPIO46 (PA_CTRL) 未初始化**：功放控制引脚没有拉高，no-std 版本有此操作。

**修复后**：Peak 值出现了非零数据，但 WakeNet 仍然无法检测唤醒词。问题进入第二阶段。

---

## 第二阶段：I2S 音频数据稀疏（84% 为零）——最艰难的排查过程

这是整个调试过程中耗时最长、走了最多弯路的阶段。

### 现象

I2S 读取的 1024 个 i16 采样中，只有约 160 个非零值（`non_zero=160/1024`）。音频数据呈现规律性的稀疏模式，例如 `first8=[0, -910, -910, -17920, 0, 0, 0, 0]`——每 8 个采样中只有 2-3 个有值。WakeNet/AFE 需要密集的 16kHz 单声道数据，稀疏数据无法触发唤醒。

### 尝试 1：StdConfig Philips 槽宽修正 → 失败

**假设**：`StdSlotConfig::philips_slot_default()` 的默认 `slot_bit_width` 是 `Auto`（实际为 32-bit），而 ES7210 输出 16-bit 数据。32-bit 槽位的高 16 位为零，导致稀疏。

**操作**：
```rust
let slot_config = StdSlotConfig::philips_slot_default(DataBitWidth::Bits16, SlotMode::Stereo)
    .slot_bit_width(SlotBitWidth::Bits16);
```

**结果**：稀疏模式完全不变。`first8` 仍然是同样的零值分布。

### 尝试 2：切换到 TDM-4 模式 → 部分成功但仍稀疏

**假设**：ES7210 是四通道 ADC，可能默认以 TDM 模式输出（4 个 slot 交织）。STD Philips 模式只看 2 个 slot，导致大量数据被丢弃。

**操作**：从 `new_std_bidir` 切换到 `new_tdm_bidir`，配置 4 个 slot：
```rust
let slot_config = TdmSlotConfig::philips_slot_default(DataBitWidth::Bits16, SlotMode::Stereo)
    .total_slots(4);
let clk_config = TdmClkConfig::from_sample_rate_hz(16000)
    .mclk_multiple(MclkMultiple::M256);
```

**结果**：
- 可以看到 4 个通道（S0-S3）各自的 peak 值，S1/S3 像是麦克风数据
- 但 feed 给 AFE 的单声道数据**依然稀疏**（`first8` 中只有 2/8 非零，且有重复值）
- 推测有效采样率只有约 4kHz 而非 16kHz

### 尝试 3：TDM-2 模式 → 失败

**假设**：TDM-4 的 BCLK = 1024kHz（4ch × 16bit × 16kHz），而 no-std 的 esp-hal `Data16Channel16` 对应 BCLK = 512kHz（2ch × 16bit × 16kHz）。BCLK 翻倍可能导致 ES7210 采样对齐问题。改为 TDM-2 让 BCLK 回到 512kHz。

**操作**：TDM slot mask 改为 `Slot0 | Slot1`，移除 `total_slots(4)`。

**结果**：`first8=[0,-910,-910,-17920,0,0,0,0]`，稀疏依旧。

### 尝试 4：SlotBitWidth::Bits16 on TDM → 失败

**假设**：TDM 模式下 `Auto` 可能也解析为 32-bit slot。

**操作**：TDM config 加上 `.slot_bit_width(SlotBitWidth::Bits16)`。

**结果**：无变化。

### 尝试 5：TX/RX 线程分离 → 失败

**假设**：同一线程交替调用 `i2s_tx.write()` 和 `i2s_rx.read()` 可能导致 RX FIFO 饿死（TX 写入阻塞时 RX 数据溢出丢失）。

**操作**：将 TX silence 写入移到独立线程，主线程只做 RX 读取。

**结果**：稀疏位置略有偏移，但非零比例不变。排除 TX/RX 竞争。

### 尝试 6：软件累积 + 去重 → 失败

**假设**：既然 I2S 每次只给约 160 个有效采样，不如累积多次读取直到凑够 512 个非零采样再 feed 给 AFE。同时去重连续重复值。

**操作**：实现了累积缓冲区 + 连续去重逻辑。

**结果**：去重后有效采样更少（~1kHz 独立采样值），AFE ringbuffer 频繁报空。这条路完全不通——问题在于硬件层没给出正确数据，软件层怎么补都没用。

### 尝试 7：切回 StdConfig Stereo + echokit_box 参考 → 部分回退

**转折**：用户提供了 `echokit_box` 开源项目和官方 ESP-BOX-3 BSP 代码作为参考。BSP 中 `bsp_audio_init()` 使用的是 **`i2s_std_config_t`**（STD 模式），不是 TDM。这与之前"ES7210 必须 TDM"的假设矛盾。

**操作**：回退到 `StdConfig` + Stereo + `SlotBitWidth::Bits16`，保留 DMA 参数 `dma_buffer_count(6)` + `frames_per_buffer(512)`。

**结果**：`non_zero=160/1024` 的稳定稀疏模式——和最初一样。但至少确认了 STD 模式是正确的 API 路径。

### 尝试 8：MCLK 倍频 M256 → M384 → 无效

**假设**：MCLK 频率不匹配导致 ES7210 内部 ADC 采样时钟错误。ES7210 的系数表中 16kHz 对应 256fs（MCLK=4.096MHz），当前用 M256 应该正确，但尝试 M384（6.144MHz）看看。

**操作**：`MclkMultiple::M256` → `MclkMultiple::M384`。

**结果**：稀疏模式完全不变。排除 MCLK 倍频问题（后来确认 M256 才是正确值）。

### 尝试 9：`nz_pos` 位置分析 → 发现周期性

**操作**：添加详细诊断日志，记录非零采样的索引位置 `nz_pos=[2,3,4,5,6,34,35,36,37,38,...]`。

**发现**：非零值呈现**严格的周期性** —— 每 32 个 i16 位置中，连续 5 个有值、其余 27 个为零。这个 "32 周期" 模式非常规律。

**错误推断**：怀疑 I2S 外设被配置为 16 个 TDM slot（32 个 i16 = 16 slot × 2 byte），内部只有 2 个 slot 有数据。但 STD 模式不应该有 16 slot。

### 尝试 10：I2S 硬件寄存器 Dump → 第一次用错基地址

**操作**：通过 `unsafe` 直接读取 I2S 寄存器，验证硬件实际配置。

**第一次错误**：使用了 `0x6002_D000` 作为 I2S0 基地址（这是 I2S1 的地址），所有寄存器读出全是 `0x00000000`。

**修正**：查阅 ESP32-S3 技术手册，I2S0 正确基地址为 **`0x6000_F000`**，并使用 `i2s_reg.h` 中的寄存器偏移。

**第二次结果**：
```
RX_TDM_CTRL = 0x00010003
  → tot_chan_num = 1 (2 channels)
  → chan_en = 0b11 (slot 0 + slot 1 enabled)
```

**关键结论**：I2S 外设本身配置完全正确——2 通道 STD 模式，没有 16 slot 的问题。稀疏的根因不在 I2S 外设，而在上游的 ES7210 codec。

### 最终突破：对比官方 BSP 的 ES7210 驱动

**操作**：逐个寄存器对比我们的 `es7210_init()` 和官方 ESP-BOX-3 BSP 中的 ES7210 驱动代码，并参照 ES7210 数据手册。

**发现了核心问题**：

| 寄存器 | 我们的值 | 官方 BSP 值 | 含义 |
|--------|---------|------------|------|
| **`0x11`** | **`0x00`** | **`0x60`** | **I2S 数据位宽：`0x00`=24-bit, `0x60`=16-bit** |
| `0x07` (OSR) | `0x00` | `0x20` | 过采样率 |
| `0x02` (MAINCLK) | 旧值 | `0xC1` | ADC divider + doubler + DLL |
| `0x09` (TIME_CTRL0) | 缺失 | `0x30` | 上电时序 |
| `0x0A` (TIME_CTRL1) | 缺失 | `0x30` | 上电时序 |

**根因**：寄存器 `0x11` 设为 `0x00` 意味着 ES7210 以 **24-bit I2S 模式**输出数据，而 ESP32-S3 I2S 外设期望 **16-bit** 数据。24-bit 数据塞进 16-bit 的帧结构中，导致：
- 每个 24-bit 采样跨越了多个 16-bit 槽位
- 高 8 位的零和低位数据在 16-bit 对齐后产生规律性的零值
- 最终呈现出 "每 32 个位置只有 5 个非零" 的稀疏模式

**陷阱**：`0x11 = 0x00` 看起来像 "默认/关闭"，但在 ES7210 的位域定义中，bit[6:5] = `00` 对应 24-bit，`11` 对应 16-bit。必须查数据手册的位域表，不能想当然。

### 修复

完全重写 `es7210_init()`，对齐官方 ESP-BOX-3 BSP：

```rust
// 关键寄存器修复
write(0x11, 0x60)?;  // 16-bit I2S（之前是 0x00 = 24-bit！）
write(0x07, 0x20)?;  // OSR（之前是 0x00）
write(0x02, 0xC1)?;  // MAINCLK: adc_div + doubler + DLL
write(0x09, 0x30)?;  // 上电时序
write(0x0A, 0x30)?;  // 上电时序
```

同时 MCLK 倍频回到 `M256`（匹配 ES7210 系数表 256fs/16kHz = 4.096MHz）。

**修复后**：`non_zero = 1024/1024`（100% 密集），WakeNet 立即开始连续检测到唤醒词。设备端音频采集彻底修复。

### 反思

这个阶段之所以走了大量弯路，核心原因是：
- **底层假设错误**：一直以为问题在 I2S 配置（STD/TDM 模式、槽宽、MCLK 等），实际问题在 codec 的 I2S 输出格式
- **no-std 参考的误导**：no-std 版本的 `es7210_init` 中 `0x11` 也是 `0x00`，但 no-std 使用 `esp-hal` 的 `Data16Channel16` 可能在底层有不同的帧对齐处理
- **诊断方法演进**：从 `first8` 粗略观察 → `non_zero` 统计 → `nz_pos` 位置分析 → I2S 寄存器 dump，最终才定位到需要去看 codec 侧的寄存器

---

## 第三阶段：设备端唤醒无限重复触发

### 现象

说一次 "Hi ESP" 后，设备不断重复发送 VOICE_START → VOICE_DATA → VOICE_END 循环，即使不再说唤醒词。服务端收到大量碎片化的音频 session。

### 根因

AFE fetch 循环中使用 `res.wake_word_index > 0` 判断唤醒：
```rust
// 错误
if res.wake_word_index > 0 && !wakeup_triggered { ... }
```

`wake_word_index` 是"上次检测到的唤醒词 ID"，一旦触发就**保持非零**不会归零。而 `wakeup_triggered` 在每次语音结束时重置为 `false`，导致下一轮 fetch 立即重新满足条件。

### 修复

```rust
// 正确：使用瞬态标志
if res.wakeup_state != 0 && !wakeup_triggered { ... }
```

`wakeup_state` 只在检测到唤醒词的那一帧为非零，之后自动回零。

### 教训

ESP-SR 的 `afe_fetch_result_t` 中两个字段用途完全不同：
- `wake_word_index`：持久值，表示"最后一次检测到的唤醒词编号"
- `wakeup_state`：瞬态值，表示"本帧是否刚发生唤醒事件"

---

## 第四阶段：服务端 STT 输出完全错乱

### 现象

服务端 STT 输出与实际语音毫无关系，如 "哎呀屁!"、"阿尔斯"。

### 根因

`local_stt.rs` 的 `transcribe()` 函数将相邻两个 i16 采样求平均：
```rust
// 错误：把单声道当立体声处理
for chunk in pcm_data.chunks_exact(2) {
    let mono = (chunk[0] as f32 + chunk[1] as f32) / 2.0;
    f32_samples.push(mono / 32768.0);
}
```

设备端 AFE 输出的已经是单声道数据，这段代码把连续两个采样当成左/右声道合并，导致音频时长减半、波形严重失真。

### 修复

```rust
// 正确：直接逐采样转换
let f32_samples: Vec<f32> = pcm_data
    .iter()
    .map(|&s| (s as f32) / 32768.0)
    .collect();
```

### 教训

在音频处理链路中，每一步数据的格式（声道数、采样率、位深）必须明确传递和校验。AFE 输出的是单声道 16kHz 16-bit PCM，不需要立体声降混。

---

## 第五阶段：STT 幻觉——识别结果包含不存在的内容

### 坑 5a：Rolling Buffer 注入 ~3 秒旧音频噪声

**现象**：说 "音量调大"，STT 输出 "铁锁定屏幕,铁锁定屏幕,音量调大"。

**根因**：服务端 `audio.rs` 的 `start_manual_session()` 在开始录音时，先将 rolling buffer 中 ~3 秒的历史音频写入 WAV 文件。这个 rolling buffer 是为服务端 VAD 设计的（需要语音前上下文），但设备端 AFE 触发模式下，`MSG_VOICE_DATA` 已经是有效语音，pre-buffer 里全是静音/噪声/上次命令的残留。

**修复**：设备触发的 session 不再写入 rolling buffer，直接清空：
```rust
let writer = WavWriter::create(&full_path, self.spec)?;
self.rolling_buffer.clear();
```

### 坑 5b：`* 2.5` 增益导致削波失真

**现象**："静音" 被识别为 "进入户口"、"敬烟"、"尽盐"。长句相对准确，短命令（2 个音节）严重失真。

**根因**：`local_stt.rs` 中有历史遗留的增益放大 `* 2.5`，这是 ES7210 还在 24-bit 错误模式时为了补偿微弱信号加的。修正 codec 后，正常语音峰值达 50%-80% 满幅，`* 2.5` 会将大量采样 clamp 到 ±1.0（方波失真）。

**修复**：去掉增益，使用标准归一化 `(s as f32) / 32768.0`。

### 坑 5c：`initial_prompt` 列表格式导致 Whisper 复读

**现象**：说 "音量调大"，STT 输出 "锁定屏幕, 音量调大, 音量调大"。

**根因**：为了提升识别率，把所有命令以逗号列表形式放进了 `initial_prompt`。Whisper 把它当作"之前的转录文本"，在低信噪比段会延续列表模式，直接复读 prompt 内容。

**修复**：改为叙述性句子，提供词汇上下文但不创建可延续的列表模式。

### 坑 5d：Whisper 输出繁体中文

**现象**："锁定屏幕" → "鎖定屏幕"，"取消静音" → "取消靜音"。

**根因**：Whisper medium 训练数据含大量繁体中文，`set_language("zh")` 不区分简繁。

**修复**：在 `initial_prompt` 中加入简体中文文本引导输出风格：
```rust
params.set_initial_prompt("以下是简体中文语音指令。Hi ESP，");
```

Whisper 的解码器会跟随 conditioning text 的书写风格。无需后处理字符映射。

---

## 最终配置汇总

### 设备端（hub-device-std）

| 组件 | 配置项 | 最终值 | 说明 |
|------|--------|--------|------|
| ES7210 | Reg `0x11` | `0x60` | 16-bit I2S 格式 |
| ES7210 | Reg `0x07` (OSR) | `0x20` | 过采样率，匹配 16kHz |
| ES7210 | Reg `0x02` (MAINCLK) | `0xC1` | ADC divider + doubler + DLL |
| ES7210 | Reg `0x09/0x0A` (TIME_CTRL) | `0x30` | 正确上电时序 |
| I2S | 模式 | StdConfig Philips Stereo | 非 TDM |
| I2S | MclkMultiple | `M256` | 匹配 ES7210 系数表 256fs |
| I2S | SlotBitWidth | `Bits16` | 匹配 ES7210 16-bit 输出 |
| AFE fetch | 唤醒判断 | `res.wakeup_state != 0` | 瞬态标志，非持久 ID |

### 服务端（hub-server）

| 组件 | 配置项 | 最终值 | 说明 |
|------|--------|--------|------|
| Whisper | 模型 | `ggml-medium.bin` | 中文短命令识别最佳平衡 |
| Whisper | beam_size | `3` | 短指令无需大 beam，提升速度 |
| Whisper | suppress_blank | `true` | 抑制静音段幻觉 |
| Whisper | initial_prompt | `"以下是简体中文语音指令。Hi ESP，"` | 引导简体 + 唤醒词词汇 |
| Whisper | 增益 | `/ 32768.0`（无额外增益） | 标准归一化，避免削波 |
| Whisper | no_context | `true` | 阻断段间上下文避免重复 |
| Audio | Rolling buffer | 清空，不写入 WAV | 设备触发模式不需要 pre-buffer |
| Audio | 采样转换 | 逐采样 i16→f32 | 不做立体声降混 |

---

## 关键经验总结

1. **Codec 寄存器的"零值"不等于"默认"**：ES7210 `0x11=0x00` 看起来无害，实际是 24-bit 模式。永远查数据手册的位域定义。

2. **诊断要分层递进**：`first8`（粗看）→ `non_zero` 统计（量化）→ `nz_pos` 位置（找规律）→ 硬件寄存器 dump（确认外设状态）→ 对比参考实现（定位 codec）。

3. **参考实现要对比正确的层**：no-std 版本的 codec 寄存器表不一定能直接照搬到 std 版本——底层 HAL 的帧对齐行为可能不同。官方 BSP（C 实现）是更可靠的参考。

4. **临时补丁必须及时清理**：`* 2.5` 增益在 codec 修复后变成了灾难。每个临时修复都应标注"依赖条件"和"移除时机"。

5. **音频链路的每一环都要确认数据格式**：声道数、采样率、位深在每次传递时都应明确校验。单声道被当成立体声处理，后果是毁灭性的。

6. **Whisper `initial_prompt` 是双刃剑**：它不是"关键词列表"，而是"模拟的历史转录文本"。列表格式会导致复读，叙述性句子才能正确引导。

7. **`wakeup_state` vs `wake_word_index`**：ESP-SR 文档对此区分不够清晰。前者是事件（瞬态），后者是状态（持久）。做唤醒判断必须用事件。

8. **当软件补丁越来越复杂时，回头看硬件配置**：累积缓冲区、去重、数据压缩——这些软件 workaround 的复杂度爆炸时，往往说明根因在更底层。
