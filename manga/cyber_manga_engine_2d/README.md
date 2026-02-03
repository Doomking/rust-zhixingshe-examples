# Cyber Manga Engine 2D (CME)

**“拒绝云端！用纯 Rust + Candle 打造的 2D 漫剧生产流水线”**

本项目并非仅仅是一个 AI 绘图工具，而是一个致力于实现 **“剧本 -> 漫画”** 全自动化生成的本地引擎。它基于 Rust 的高性能生态（Candle, Axum, Image），旨在为创作者提供一个从**脚本解析**到**分镜生成**再到**最终排版**的一站式解决方案。

> 📚 **设计哲学**: 详见 [doc.md](./doc.md) —— 本项目不仅追求图像生成，更致力于探索 2D -> 2.5D -> 3D 的漫剧进化路线。

## 🌟 核心愿景与路线图 (Roadmap)

本项目按照「漫剧进化」三阶段进行开发：

### ✅ 第一阶段：2D 自动化流水线 (当前核心)
*   [x] **硬核推理底座**: 基于 `Candle` 框架的纯 Rust 本地 Stable Diffusion 推理。
*   [x] **高性能服务**: `Axum` + `Tokio` 异步后端，支持 GPU (Metal/CUDA) 加速。
*   [x] **交互式驾驶舱**: `React` + `Vite` 构建的现代化暗色系操作界面。
*   [ ] **智能剧本解析** (Coming Soon): 集成 LLM (如 Qwen/Llama)，自动将自然语言剧本转换为分镜描述。
*   [ ] **自动嵌字排版** (Coming Soon): 利用 `imageproc` 自动计算气泡位置并填充对话文字。

### ⏳ 第二阶段：2.5D 呼吸感 (规划中)
*   **深度图估计**: 生成 Depth Map，实现伪 3D 效果。
*   **视差滚动**: 让漫画分镜在鼠标交互下产生“呼吸感”。

### ⏳ 第三阶段：3D 赛博剧场 (规划中)
*   **3D Gaussian Splatting**: 生成全 3D 场景。
*   **实时演出**: 语音驱动口型 (Lip-sync)。

---

## 📂 项目结构

| 目录 | 模块名 | 说明 |
| :--- | :--- | :--- |
| `engine/` | **Engine** | **核心引擎**。负责 AI 模型加载、推理调度、图像后处理。不依赖 Python。 |
| `cockpit/` | **Cockpit** | **驾驶舱**。创作者的操作界面，负责剧本输入、生成控制和结果预览。 |

## 🚀 快速启动 (Quick Start)

体验当前的**核心渲染引擎**功能：

### 1. 启动后端引擎
```bash
cd engine
# 首次运行将自动下载 Stable Diffusion v1.5 模型 (FP16)
cargo run --bin engine
```
*等待日志出现 `Models loaded successfully!`*

### 2. 启动前端驾驶舱
```bash
cd cockpit
npm install
npm run dev
```
*打开浏览器访问 http://localhost:5173*

## 📖 操作指南

目前版本已开放 **“原画生成”** 核心功能：

1.  在驾驶舱输入提示词 (Prompt)，例如描述一个赛博朋克风格的场景。
2.  点击 **Generate**，引擎将调用本地显卡进行高速推理。
3.  生成的图片将作为漫剧的基础分镜素材。

## 🛠️ 技术栈

*   **Language**: Rust 🦀 (后端), TypeScript (前端)
*   **AI Framework**: [Hugging Face Candle](https://github.com/huggingface/candle) (无 Python 依赖)
*   **Web Framework**: Axum (后端), React + Vite (前端)
*   **Graphics**: wgpu (底层加速), image-rs (图像处理)

---
**License**: MIT
