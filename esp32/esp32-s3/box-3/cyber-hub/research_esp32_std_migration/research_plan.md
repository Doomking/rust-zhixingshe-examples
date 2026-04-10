# Research Plan: ESP32-S3 Rust std vs no_std & ESP-SR Integration

## Main Research Question
What are the technical implications, benefits, and costs of migrating the Cyber-Hub project from `no_std` (esp-hal) to `std` (esp-idf-hal) to support on-device wake-word detection (ESP-SR)?

## Subtopics
1. **`std` vs `no_std` Technical Comparison**:
   - Memory overhead (PSRAM usage).
   - Boot speed.
   - Compatibility of existing drivers (display, I2C sensors).
2. **ESP-SR Rust Integration**:
   - How to call `esp-sr` (AFE/WakeNet) from Rust.
   - Existence of generic wrappers or documented FFI examples.
3. **Migration Effort for Peripherals**:
   - Refactoring `esp-hal` SPI/I2C/I2S calls to `esp-idf-hal`.
   - Handling Embassy-style async tasks in an ESP-IDF (FreeRTOS) environment.

## Expected Information
- A clear list of "pros and cons" for the user.
- A technical feasibility confirm for "Bottom Layer" wake-word detection.
- A high-level refactoring map for existing functions (UI, Sensors).
