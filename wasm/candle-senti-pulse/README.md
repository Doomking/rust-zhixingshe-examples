为了匹配你现在的 **Candle + Wasm + 中文情绪识别** 项目，我为你重新设计了一份更具实战指导意义的 `README.md`。

它不仅保留了原始模板的核心功能，还重点增加了你要求的 **Python 模型转换流程** 和 **本地部署说明**。

---

# AI 情绪中枢 (Candle + Wasm) 🦀🕸️

本项目是一个基于 Rust `candle` 框架和 WebAssembly 的前端实时情绪分析系统。通过浏览器端的推理引擎，实时捕捉文本中的 Positive、Negative 和 Neutral 情绪。

## 🏗️ 项目结构

* `src/`: Rust 推理引擎源代码。
* `model2safetensor/`: Python 模型下载与格式转换工具 (基于 `uv` 管理)。
* `www/`: 前端静态资源，包含粒子风暴 UI 和 WASM 调用逻辑。

---

## 🚀 快速开始

### 1. 模型准备 (Python 环境)

由于模型文件较大，不进入 Git 追踪。我们需要将 Hugging Face 的模型转换为 WASM 兼容的 `.safetensors` 格式。

我们将使用 **[uv](https://docs.astral.sh/uv/)** 快速运行转换脚本：

```bash
cd model2safetensor

# 运行转换脚本：自动下载权重、导出 config.json 和 tokenizer.json
uv run main.py

```

转换完成后，模型文件会生成在 `converted_model/` 目录下。

### 2. 部署模型文件

手动将生成的模型资源移动到 Web 服务的访问路径下：

```bash
# 创建目标目录（确保符合 .gitignore 规则）
mkdir -p ../www/model/sentiment-zh/

# 复制必要文件
cp converted_model/* ../www/model/sentiment-zh/

```

### 3. 构建 WebAssembly 核心

确保你已安装 `wasm-pack`。

```bash
# 在项目根目录下执行
wasm-pack build --target web

```

### 4. 启动本地服务器

我们推荐使用 `miniserve` 来处理 WASM 的 MIME 类型和静态资源访问：

```bash
# 在项目根目录启动
miniserve ./ -p 8080 --index www/index.html

```

访问地址：[http://127.0.0.1:8080/www/](https://www.google.com/search?q=http://127.0.0.1:8080/www/)

---

## 🛠️ 开发与配置

### JS 路径更新

如果在转换时更改了模型名称，请在 `www/main.js` 中同步更新路径：

```javascript
const baseUrl = "./model/sentiment-zh/";

```

### 调试与测试

* **Rust 测试**: `wasm-pack test --headless --firefox`
* **情绪压力测试**: 页面下方提供了“情绪压力测试集”，支持一键填入预设文本验证模型鲁棒性。

---

## 🔋 技术栈

* **Rust & Candle**: 轻量级深度学习框架。
* **Wasm-pack**: 打包 Rust 为高效率浏览器模块。
* **UV**: Python 依赖的高速管理工具。
* **Canvas API**: 实时渲染受情绪波动驱动的粒子物理系统。

## 📜 许可证

本项目遵循 [MIT](https://www.google.com/search?q=LICENSE-MIT) 或 [Apache-2.0](https://www.google.com/search?q=LICENSE-APACHE) 双重许可。

---

### 💡 建议提示

由于你已经在 `.gitignore` 中忽略了 `**/model/`，请确保在部署到生产服务器（如 Vercel 或自己的 VPS）时，通过部署脚本或 CI/CD 流水线重新执行模型转换或下载步骤。

**需要我帮你写一个简单的 `setup.sh` 脚本，把上述所有模型下载、移动、构建的步骤自动化吗？**
