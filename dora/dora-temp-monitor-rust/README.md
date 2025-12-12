
# 一个基于 Dora.rs 的混合语言（Rust + Python）实时温度监控系统，实现了温度数据的模拟、处理、异常检测和可视化。

## 项目结构

```
dora-temp-monitor-rust/
├── sensor-node/          # Rust 温度传感器模拟节点
├── processor-node/       # Rust 数据处理和异常检测节点
├── logger-node/          # Rust 日志和终端可视化节点
├── visualizer-node/      # Python 可视化节点
├── dataflow.yml          # Dora 数据流配置文件
└── Cargo.toml            # Rust 项目依赖管理
```

## 功能特性

- **实时温度模拟**：传感器节点生成带噪声和趋势的模拟温度数据
- **数据平滑处理**：使用滑动窗口算法对温度数据进行平滑处理
- **异常检测**：检测温度突变并发出警报
- **多端可视化**：
  - 终端柱状图实时显示温度变化
  - Python 可视化节点提供更丰富的图表展示
- **模块化设计**：基于 Dora.rs 的节点化架构，易于扩展和维护

## 节点说明

### 1. 传感器节点 (sensor-node)
- 用 Rust 编写，生成模拟温度数据
- 添加随机噪声和正弦趋势，模拟真实传感器行为
- 每 100ms 发送一次温度数据

### 2. 处理器节点 (processor-node)
- 用 Rust 编写，实现数据处理逻辑
- 滑动窗口平滑算法（窗口大小：10）
- 异常检测（温差阈值：3°C）
- 输出平滑后的温度数据和异常警报

### 3. 日志节点 (logger-node)
- 用 Rust 编写，终端可视化
- 实时显示温度柱状图
- 打印异常警报信息

### 4. 可视化节点 (visualizer-node)
- 用 Python 编写，提供更丰富的可视化
- 可扩展的可视化界面

## 安装与运行

### 前提条件
- Rust (>= 1.70.0)
- Python 3.10+

### 安装步骤

1. **安装 Rust 依赖管理工具**
   ```bash
   cargo install uv
   ```

2. **安装 Python 节点依赖**
   ```bash
   cd visualizer-node && uv install
   ```

### 启动系统

1. **激活 Python 虚拟环境**
   ```bash
   source visualizer-node/.venv/bin/activate
   ```

2. **启动 Dora 系统**
   ```bash
   dora up
   ```

3. **构建数据流**
   ```bash
   dora build dataflow.yaml
   ```

4. **启动数据流**
   ```bash
   dora start dataflow.yaml
   ```

### 停止系统

```bash
# 停止数据流
dora stop

# 关闭 Dora 系统
dora down

# 退出虚拟环境
deactivate
```

## 数据流配置

数据流定义在 `dataflow.yml` 文件中，描述了四个节点之间的连接关系：

- **temp_sensor** → **data_processor**：发送原始温度数据
- **data_processor** → **logger**：发送平滑温度和异常警报
- **data_processor** → **visualizer**：发送平滑温度数据进行可视化

## 技术栈

- **Rust**：高性能节点实现
- **Python**：可视化节点
- **Dora.rs**：数据流转和节点通信框架
- **Arrow**：高效数据格式

## 扩展说明

1. **添加新节点**：创建新的 Rust 或 Python 项目，添加到 dataflow.yml
2. **修改参数**：
   - 传感器采样频率：修改 dataflow.yml 中的 timer 配置
   - 滑动窗口大小：修改 processor-node 中的 window_size
   - 异常阈值：修改 processor-node 中的 threshold

## 故障排除

- **找不到 Python 路径**：确保已激活虚拟环境 `source visualizer-node/.venv/bin/activate`
- **编译错误**：检查 Rust 版本是否符合要求，运行 `cargo build` 验证
- **连接问题**：确保 Dora 系统已启动 `dora up`

## 许可证

MIT

        