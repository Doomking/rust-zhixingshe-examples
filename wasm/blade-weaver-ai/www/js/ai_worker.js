// workers/ai_worker.js
import init, { GameNarrator } from "../../pkg/blade_weaver_ai.js";

let narrator = null;
let isModelReady = false;
let lastNarratedTime = 0;
const NARRATION_COOLDOWN = 3000; // 冷却时间：3秒内不重复说话，避免吵闹

async function fetchBinary(url) {
  const response = await fetch(url);
  const buffer = await response.arrayBuffer();
  return new Uint8Array(buffer);
}

// 1. 初始化模型
async function loadModel() {
  try {
    console.log("🤖 AI 正在加载语言中枢...");
    // 1. 初始化 WASM 模块
    await init();

    // 2. 定义模型托管路径（建议放在项目的 public/model 下，或使用 HuggingFace URL）
    const MODEL_BASE = "../assets/model/Qwen2.5-0.5B-Instruct/";

    console.log("📥 开始下载 AI 核心文件...");

    // 3. 并行下载：利用浏览器带宽优势
    const [weights, tokenizer, config] = await Promise.all([
      fetchBinary(`${MODEL_BASE}model.safetensors`),
      fetchBinary(`${MODEL_BASE}tokenizer.json`),
      fetchBinary(`${MODEL_BASE}config.json`),
    ]);

    // 4. 调用 Rust 导出的初始化接口
    // 内部调用了 LLMEngine::init(weights_data, tokenizer_data, config_data)
    narrator = await GameNarrator.new(weights, tokenizer, config);

    console.log("✅ AI 导演已就位！");
    isModelReady = true;
    self.postMessage({ type: "status", text: "AI 已就绪，刀锋准备就绪！" });
  } catch (err) {
    console.error("AI 加载失败:", err);
    self.postMessage({ type: "error", text: "AI 加载失败" });
  }
}

// 2. 消息处理中心
self.onmessage = async (e) => {
  const { type, event } = e.data;

  if (type === "game_event") {
    if (!isModelReady) return;

    // 策略控制：不是每个动作都要解说，只挑选“高光时刻”
    if (shouldNarrate(event)) {
      const commentary = await generateCommentary(event);
      self.postMessage({ type: "commentary", text: commentary });
    }
  }
};

// 3. 智能决策：判断是否需要生成解说
function shouldNarrate(event) {
  const now = Date.now();
  if (now - lastNarratedTime < NARRATION_COOLDOWN) return false;

  // 只有在特定事件发生时才解说
  switch (event.action) {
    case "game_start":
      return true;
    case "slice":
      // 比如：只有连击或者是特定的“完美切割”才解说
      return Math.random() > 0.7;
    case "combo":
      return true;
    default:
      return false;
  }
}

// 4. 调用 Rust 核心生成文本
async function generateCommentary(event) {
  lastNarratedTime = Date.now();

  // 将游戏事件序列化，传给 Rust 层的逻辑
  // Rust 会根据事件类型匹配不同的 System Prompt
  const eventJson = JSON.stringify(event);
  console.log("🎤 生成解说，事件数据：", eventJson);
  // 调用我们在 Rust lib.rs 中写的核心函数
  const text = await narrator.process_game_event(eventJson);

  return text;
}

// 执行加载
loadModel();
