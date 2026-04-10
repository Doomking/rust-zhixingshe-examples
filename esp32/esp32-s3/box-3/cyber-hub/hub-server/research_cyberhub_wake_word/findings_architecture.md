# Research Plan - ESP32 Voice Assistant Architectures

## Tasks
- [x] Search 1: ESP-SR speech recognition pipeline architecture (AEC, NS, VAD, WakeNet)
- [x] Search 2: Willow voice assistant architecture (Willow by Tovera) - audio streaming and wake word
- [x] Search 3: ESPHome/Home Assistant voice assistant pipeline architecture
- [x] Task 4: Determine industry standard for wake word detection (on-device vs. server)
- [x] Task 5: Save findings to `research_cyberhub_wake_word/findings_architecture.md` (Note: Put in scratchpad due to file access restrictions)

## ESP32 Voice Assistant Architecture Research

### 1. ESP-SR (Espressif Standard) Pipeline
The official Espressif speech recognition framework (ESP-SR) follows a modular pipeline:
- **Audio Front End (AFE):**
    - **AEC (Acoustic Echo Cancellation):** Cancels the far-end signal (e.g., music playing from the speaker) from the microphone input.
    - **NS (Noise Suppression):** Reduces steady-state background noise.
    - **AGC (Automatic Gain Control):** Adjusts the volume of the recorded voice.
    - **VAD (Voice Activity Detection):** Determines if someone is speaking.
- **WakeNet (Wake Word Engine):**
    - High-performance on-device wake word detection.
- **MultiNet (Speech Command Recognition):** Recognizes pre-defined commands on-device.

### 2. Willow Voice Assistant Architecture
Willow is a high-performance open-source voice assistant using the ESP32-S3 Box.
- **Hybrid Approach:** On-device wake word/VAD + Server-side STT (Whisper).
- **Communication:** Streams audio to **Willow Inference Server (WIS)** using **Opus** or raw audio via **WebSockets**.
- **Performance:** Sub-500ms response time.

### 3. ESPHome / Home Assistant (microWakeWord)
- **Model:** Uses **TensorFlow Lite Micro** for quantized, streaming Inception-based models on ESP32-S3.
- **Accuracy:** Better than ESP-SR's default models due to modern architecture and training.
- **Pipeline:** Wake word on ESP32 -> Stream audio over native API (Protobufs/WebSockets) to HA server.

### 4. Industry Standard & Recommendations
- **On-Device Wake Word:** INDUSTRY STANDARD. Necessary for privacy and latency.
- **AFE is Critical:** Accuracy issues are often caused by poor AEC or NS. If the mic picks up too much noise/echo, the model fails.
- **Server-Side STT:** For generic commands, streaming to **Whisper** (fast-whisper) is the gold standard.

### Recommendations for "Xiao Guang" (小光):
1. **Enable/Tune AFE:** Verify AEC and NS settings to ensure clear voice signal.
2. **Explore microWakeWord:** Consider using the microWakeWord architecture for better wake word accuracy on ESP32-S3.
3. **Audio Pre-processing:** Use hardware-accelerated AFE for best performance.
