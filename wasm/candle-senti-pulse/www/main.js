import init, { SentiPulseEngine } from "../pkg/candle_senti_pulse.js";

// =========================================
// PART 1: 粒子风暴系统 (Particle System)
// =========================================
const canvas = document.getElementById("particle-canvas");
const ctx = canvas.getContext("2d");

// 设置画布大小
function resizeCanvas() {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
}
window.addEventListener("resize", resizeCanvas);
resizeCanvas();

// 粒子参数全局状态 (受 AI 情绪驱动)
let globalMood = {
  neg: 0.1, // 初始平静状态
  pos: 0.9,
  neu: 0.1,
  targetSpeed: 0.5,
  currentSpeed: 0.5,
  chaos: 0.2, // 混乱度
};

class Particle {
  constructor() {
    this.reset();
    this.y = Math.random() * canvas.height; // 初始随机分布
  }

  reset() {
    this.x = Math.random() * canvas.width;
    this.y = canvas.height + Math.random() * 100; // 从底部生成
    this.size = Math.random() * 2 + 1;
    // 基础速度 + 随机扰动
    this.baseSpeedY = Math.random() * 1 + 0.5;
    this.vx = (Math.random() - 0.5) * 0.5;
    this.vy = -this.baseSpeedY;
    this.alpha = Math.random() * 0.5 + 0.2;
  }

  update() {
    // 根据全局情绪平滑调整当前速度
    globalMood.currentSpeed +=
      (globalMood.targetSpeed - globalMood.currentSpeed) * 0.05;

    // 情绪越消极，速度越快，水平扰动越大(混乱)
    this.x += this.vx * (1 + globalMood.chaos * 5);
    this.y += this.vy * globalMood.currentSpeed;

    // 边界检查，循环利用
    if (this.y < -10) this.reset();
  }

  draw() {
    /// 强化颜色计算：确保 neg 占主导时 R 通道强制拉满
    const r = Math.floor(globalMood.neg * 255 + globalMood.neu * 168);
    const g = Math.floor(globalMood.pos * 242 + globalMood.neu * 85);
    const b = Math.floor(globalMood.pos * 255 + globalMood.neu * 247);

    // 氛围补偿：负面越高，粒子稍微变大一点，增加压迫感
    const dynamicSize = this.size * (1 + globalMood.neg * 1.5);

    ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${this.alpha + globalMood.neg * 0.3})`;
    ctx.beginPath();
    ctx.arc(this.x, this.y, dynamicSize, 0, Math.PI * 2);
    ctx.fill();
  }
}

const particles = Array.from({ length: 150 }, () => new Particle());

function animateParticles() {
  // 使用半透明黑色清空画布，制造拖尾效果
  ctx.fillStyle = "rgba(10, 11, 16, 0.2)";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  particles.forEach((p) => {
    p.update();
    p.draw();
  });
  requestAnimationFrame(animateParticles);
}

// 启动粒子动画
animateParticles();

// =========================================
// PART 2: Wasm 推理与交互逻辑
// =========================================

// 模拟的关键词词典 (用于演示文字发光效果)
// const SIMULATED_ATTENTION = {
//     pos: ['good', 'great', 'excellent', 'amazing', 'love', 'fantastic', 'stunning', 'best', 'superb'],
//     neg: ['bad', 'worst', 'terrible', 'awful', 'hate', 'weak', 'poor', 'disaster', 'boring']
// };
// 模拟中文关键词词典
const SIMULATED_ATTENTION = {
  pos: ["好", "棒", "给力", "推荐", "喜欢", "极致", "完美", "快", "优秀", "强"],
  neg: ["差", "慢", "垃圾", "坑", "烂", "失望", "难用", "断", "死机", "废"],
};

const inputContainer = document.getElementById("user-input-container");
const heatmapStrip = document.getElementById("heatmap-strip");
// 初始化热力图格子
for (let i = 0; i < 20; i++) {
  heatmapStrip.appendChild(document.createElement("div")).className =
    "heatmap-cell";
}
let heatmapIndex = 0;

// 简单的防抖函数，避免打字时推理太频繁
function debounce(func, wait) {
  let timeout;
  return function (...args) {
    clearTimeout(timeout);
    timeout = setTimeout(() => func.apply(this, args), wait);
  };
}

// 记录上一次的纯文本，避免无意义的重绘
let lastText = "";

function highlightText(text, negProb, posProb) {
  if (text === lastText || !text.trim()) return;

  let html = text;
  let modified = false;

  // 只有情绪极化明显时才执行替换
  if (posProb > 0.6) {
    SIMULATED_ATTENTION.pos.forEach((word) => {
      const regex = new RegExp(word, "g");
      if (regex.test(html)) {
        html = html.replace(regex, `<span class="glow-pos">${word}</span>`);
        modified = true;
      }
    });
  }

  if (negProb > 0.6) {
    SIMULATED_ATTENTION.neg.forEach((word) => {
      const regex = new RegExp(word, "g");
      if (regex.test(html)) {
        html = html.replace(regex, `<span class="glow-neg">${word}</span>`);
        modified = true;
      }
    });
  }

  // 只有内容真正改变了才重绘，防止光标死循环
  if (modified && inputContainer.innerHTML !== html) {
    // 记录光标位置（稍微复杂的逻辑，但对体验至关重要）
    const selection = window.getSelection();
    const offset = selection.focusOffset;

    inputContainer.innerHTML = html;

    // 简单处理：将光标放回末尾
    placeCaretAtEnd(inputContainer);
  } else if (!modified && inputContainer.querySelector("span")) {
    // 如果没有关键词但之前有高亮，回退到纯文本
    inputContainer.innerText = text;
    placeCaretAtEnd(inputContainer);
  }

  lastText = text;
}

// 辅助：当输入为空时清空状态
function resetUI() {
  inputContainer.innerHTML = ""; // 彻底清空，让 :empty placeholder 出现
  document.getElementById("neg-mercury").style.height = "5%";
  document.getElementById("pos-mercury").style.height = "5%";
  document.getElementById("neu-mercury").style.height = "5%";
  globalMood.targetSpeed = 0.5;
}

// 将光标放置到 contenteditable 元素末尾的辅助函数
function placeCaretAtEnd(el) {
  el.focus();
  if (
    typeof window.getSelection != "undefined" &&
    typeof document.createRange != "undefined"
  ) {
    let range = document.createRange();
    range.selectNodeContents(el);
    range.collapse(false);
    let sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
  }
}

function updateVibe(result) {
  const { negative, positive, neutral } = result;
  const container = document.querySelector(".container");

  if (negative > 0.9) {
    document.body.classList.add("shake-effect");
    setTimeout(() => document.body.classList.remove("shake-effect"), 500);
  }

  // 动态调整容器边框光晕，增强氛围感
  if (positive > 0.7) {
    container.style.boxShadow = `0 20px 40px rgba(0, 242, 255, ${positive * 0.3})`;
    container.style.borderColor = `rgba(0, 242, 255, ${positive * 0.5})`;
  } else if (negative > 0.7) {
    container.style.boxShadow = `0 20px 40px rgba(255, 51, 102, ${negative * 0.3})`;
    container.style.borderColor = `rgba(255, 51, 102, ${negative * 0.5})`;
  } else {
    container.style.boxShadow = `0 20px 40px rgba(0, 0, 0, 0.4)`;
    container.style.borderColor = `rgba(255, 255, 255, 0.1)`;
  }
}

async function startApp() {
  await init();
  const status = document.getElementById("status-text");
  const stats = document.getElementById("stats-text");

  try {
    // 修改为中文模型资源
    // const baseUrl = "./model/jackietung/bert-base-chinese-finetuned-sentiment/";
    const baseUrl = "./model/uer/roberta-base-finetuned-jd-binary-chinese/";

    status.innerText = "正在接收卫星数据 (下载模型权重)...";
    // 注意：文件名可能需要根据仓库实际情况调整
    const [weights, tokenizer, config] = await Promise.all([
      fetch(baseUrl + "model.safetensors").then((r) => r.arrayBuffer()),
      fetch(baseUrl + "tokenizer.json").then((r) => r.arrayBuffer()),
      fetch(baseUrl + "config.json").then((r) => r.text()),
    ]);
    // 使用量化版模型 (Q4_K_M) 以加快加载速度，约 70MB
    // 注意：需要确保你的 miniserve 正确设置了 MIME type 或跨域头
    // const weightsUrl = "https://huggingface.co/lmz/candle-quantized-bert/resolve/main/distilbert-sst2-q4k.gguf";
    // const tokenizerUrl = "https://huggingface.co/distilbert-base-uncased-finetuned-sst-2-english/resolve/main/tokenizer.json";
    // const configUrl = "https://huggingface.co/distilbert-base-uncased-finetuned-sst-2-english/resolve/main/config.json";

    // 并发加载
    // const [weights, tokenizer, config] = await Promise.all([
    //     fetch(weightsUrl).then(r => r.arrayBuffer()),
    //     fetch(tokenizerUrl).then(r => r.arrayBuffer()),
    //     fetch(configUrl).then(r => r.text())
    // ]);

    status.innerText = "正在激活 Wasm 神经核...";
    // 注意：这里假设 Rust 端已经更新为支持 GGUF 量化加载的 new 方法
    // 如果你还在用之前的 Safetensors 版本，请回退到旧的加载 URLs 和 Rust 代码
    // 为了演示效果，我们假设 Rust 侧已经适配好了。
    const engine = new SentiPulseEngine(
      new Uint8Array(weights),
      new Uint8Array(tokenizer),
      config,
    );

    // UI 切换
    document.getElementById("loading-screen").classList.add("hidden");
    document.getElementById("main-content").classList.remove("hidden");

    // 定义推理主逻辑
    const performInference = () => {
      const text = inputContainer.innerText;
      if (!text.trim()) {
        resetUI();
        return;
      }

      const t0 = performance.now();
      // 1. 调用 Rust Wasm (假设返回对象包含 neg, pos, neu)
      const result = engine.predict(text);
      const { negative: neg, positive: pos, neutral: neu } = result;
      const t1 = performance.now();

      // --- 更新可视化 ---

      // 2. 更新温度计 (如果 HTML 有三个温度计，此处增加一个)
      document.getElementById("neg-mercury").style.height = `${5 + neg * 95}%`;
      document.getElementById("pos-mercury").style.height = `${5 + pos * 95}%`;
      // 如果你在 HTML 增加了 neu-mercury 元素：
      if (document.getElementById("neu-mercury")) {
        document.getElementById("neu-mercury").style.height =
          `${5 + neu * 95}%`;
      }

      // 3. 更新热力图条 (加入紫色通道)
      const cell = heatmapStrip.children[heatmapIndex];
      // 紫色是红+蓝，这里用 neu 来增强 B 通道和 R 通道
      cell.style.background = `rgb(${neg * 255 + neu * 100}, ${neu * 50}, ${pos * 255 + neu * 200})`;
      heatmapIndex = (heatmapIndex + 1) % 20;

      // 4. 更新统计数据
      const n_pct = Math.round((neg / (neg + pos + neu)) * 100);
      const p_pct = Math.round((pos / (neg + pos + neu)) * 100);
      // 中性值直接用 100 减去其他两项，确保总和永远是 100
      const u_pct = 100 - n_pct - p_pct;

      stats.innerText = `Inference Time: ${(t1 - t0).toFixed(1)}ms | Neg: ${n_pct}% Neu: ${u_pct}% Pos: ${p_pct}%`;
      // 5. 驱动粒子风暴
      // 中性时速度最平稳 (targetSpeed 较低)
      // globalMood.targetSpeed = 0.5 + neg * 2.5 + pos * 0.5;
      // globalMood.chaos = 0.1 + neg * 0.5;

      // 计算综合情绪分 (-1 到 1)
      // 在 performInference 函数中修改
      const sentimentScore = pos * 1 + neu * 0 + neg * -1;

      // 限制最高速度倍率，防止粒子飞得太快
      const speedMultiplier = Math.min(Math.abs(sentimentScore), 0.8);
      globalMood.targetSpeed = 0.5 + speedMultiplier * 3.0;

      // 负面情绪时，增加水平晃动量（混乱度）
      globalMood.chaos = neg * 1.2;

      // 修正全局情绪比例的对比度增强
      let n = Math.pow(neg, 1.5); // 稍微降低指数，防止 neg 吞掉所有颜色
      let p = Math.pow(pos, 2.0);
      let u = Math.pow(neu, 1.2);

      const total = n + p + u;
      globalMood.neg = n / total;
      globalMood.pos = p / total;
      globalMood.neu = u / total;

      // 6. 执行文字高亮 (可以保持原有逻辑，或增加中性词检测)
      highlightText(text, neg, pos);

      updateVibe(result);
    };

    // 监听输入，使用防抖避免过于频繁触发
    inputContainer.addEventListener("input", debounce(performInference, 500));

    // 初始执行一次
    // performInference();
  } catch (e) {
    status.innerText = "系统崩溃: " + e;
    status.style.color = "var(--neon-red)";
    console.error(e);
  }
}

startApp();
