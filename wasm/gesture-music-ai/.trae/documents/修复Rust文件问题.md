## 修复计划

### 1. 修复依赖问题
- 将 `fundsp` 和 `anyhow` 依赖从 WASM 特定目标移到通用依赖部分，确保在所有目标下都能正确解析

### 2. 修复类型不匹配问题
- 在 `lib.rs` 中，将 `Recognizer::new` 调用时的 `&[u8]` 参数转换为 `Vec<u8>`，使用 `.to_vec()` 方法

### 3. 修复字段访问错误
- 在 `lib.rs` 中，将 `self.gesture_ai.inference()` 改为 `self.recognizer.inference()`，使用正确的字段名

### 4. 修复方法不存在错误
- 在 `fluid_visual.rs` 中，为 `Simulator` 结构体添加 `get_intensity()` 方法，基于粒子数量或活跃度计算强度值

### 5. 修复未解析的函数
- 确保 `music_synth.rs` 中的 `lfo` 和 `sine` 函数能够正确解析，检查 `fundsp` 库的导入和使用方式

### 6. 验证修复
- 运行 `cargo check` 确保所有编译错误都已解决
- 验证代码能够正常编译和构建

## 具体修改文件

1. **Cargo.toml** - 调整依赖声明
2. **src/lib.rs** - 修复类型转换和字段访问
3. **src/fluid_visual.rs** - 添加缺失的方法
4. **src/music_synth.rs** - 确保函数正确解析