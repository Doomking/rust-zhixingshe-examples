# Research Plan: High-Accuracy Wake Word & Configurable Storage

## Main Research Question
What is the "best" architecture to achieve near-perfect wake word recognition for "小光" (Xiao Guang) on the ESP32-S3 Box 3 while ensuring the system configuration (like storage paths) is easily maintainable?

## Subtopics to Investigate
1. **Infrastructure**: How to best integrate environment variables for configurable file paths in a Rust tokio project.
2. **Server-Side Wake Word Engines**: Investigate [openWakeWord](https://github.com/dscripka/openWakeWord) or similar high-accuracy engines that can run on the Mac server. Can they handle "小光"?
3. **ESP32 AFE (Audio Front End)**: How to enable Hardware AEC/NS/VAD on the ESP32-S3 Box 3 to clean up the audio *before* it reaches the server.
4. **On-Device Wake Word (microWakeWord)**: Feasibility of running a small TFLite Micro model for "小光" on the ESP32-S3 in a Rust-based project.

## Expected Information
- Implementation steps for `AppConfig` to support `AUDIO_TEMP_DIR`.
- Benchmark or comparison of accuracy for openWakeWord vs. generic Whisper STT.
- Code snippets or configuration for ESP-SR / AFE on the ESP32.
- Availability of a "Xiao Guang" model for openWakeWord or microWakeWord.

## Synthesis Strategy
Propose a "Best-in-Class" architecture:
1. **Device-side**: Clean audio with AFE (AEC/NS).
2. **Server-side**: Use a dedicated wake-word engine (openWakeWord) for the trigger, then hand off to Whisper for commands.
3. **Configuration**: Use `dotenv` and a centralized `Config` struct.
