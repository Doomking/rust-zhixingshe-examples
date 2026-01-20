import init, { LLMEngine } from "../pkg/candle_llm_3d.js";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { DigitalHuman } from "./digital_human.js";
import { TextToSpeech } from "./tts.js";

async function start() {
  await init();

  // 1. 初始化场景
  const scene = new THREE.Scene();

  // 3. 动态绘制背景 (Procedural Background)
  // 不加载图片，而是用代码画一个二次元风格的网格空间
  function createProceduralBackground() {
    const canvas = document.createElement("canvas");
    canvas.width = 1024;
    canvas.height = 1024;
    const ctx = canvas.getContext("2d");

    // 2. 宫崎骏风格 (Ghibli Style) - 程序化绘制
    // 特点：高饱和度的蓝天、洁白的云朵、绿意盎然的草地

    // A. 蓝天 (Deep Blue Sky)
    const activeSky = ctx.createLinearGradient(0, 0, 0, 1024);
    activeSky.addColorStop(0, "#4A90E2"); // 天空蓝
    activeSky.addColorStop(0.6, "#87CEEB"); // 淡蓝
    activeSky.addColorStop(1, "#E0F7FA"); // 地平线白
    ctx.fillStyle = activeSky;
    ctx.fillRect(0, 0, 1024, 1024);

    // B. 云朵 (Fluffy Clouds) - 用径向渐变模拟
    function drawCloud(cx, cy, r) {
      const cloudGrad = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
      cloudGrad.addColorStop(0, "rgba(255, 255, 255, 0.95)");
      cloudGrad.addColorStop(0.4, "rgba(255, 255, 255, 0.8)");
      cloudGrad.addColorStop(1, "rgba(255, 255, 255, 0)");
      ctx.fillStyle = cloudGrad;
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
    }

    // 画几朵大云
    for (let i = 0; i < 8; i++) {
      const x = Math.random() * 1024;
      const y = Math.random() * 400; // 只在天上
      const r = 50 + Math.random() * 100;
      drawCloud(x, y, r);
      drawCloud(x + r * 0.6, y + r * 0.2, r * 0.8); // 叠加产生体积感
    }

    // C. 草地 (Green Grass)
    // 简单的起伏山坡
    ctx.fillStyle = "#A3D978"; // 嫩绿
    ctx.beginPath();
    ctx.moveTo(0, 800);
    ctx.bezierCurveTo(300, 750, 700, 850, 1024, 800);
    ctx.lineTo(1024, 1024);
    ctx.lineTo(0, 1024);
    ctx.fill();

    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    return texture;
  }

  scene.background = createProceduralBackground();

  const camera = new THREE.PerspectiveCamera(
    45,
    window.innerWidth / window.innerHeight,
    0.1,
    1000,
  );
  // 视角终合调优：改为正对前方 (Frontal View)
  // Camera X=0.2, Target X=0.2 -> 视线平行，数字人(X=0) 位于屏幕左侧
  camera.position.set(0.2, 1.0, 0.65);

  // 2. 增强光照
  const directionalLight = new THREE.DirectionalLight(0xffffff, 2.0);
  directionalLight.position.set(2, 4, 5);
  scene.add(directionalLight);

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.domElement.style.position = "absolute";
  renderer.domElement.style.top = "0";
  document.getElementById("canvas-container").appendChild(renderer.domElement);

  // 3. 添加轨道控制器 (OrbitControls)
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.enableDamping = true;
  controls.target.set(0.2, 0.85, 0); // COI 设为 0.2，让位于 0.0 的数字人显在左边
  controls.enablePan = false;
  controls.minDistance = 0.3;
  controls.maxDistance = 3.0;

  // 响应式调整
  window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
  });

  // 强化光照
  const ambientLight = new THREE.AmbientLight(0xffffff, 1.0);
  scene.add(ambientLight);

  // 2. 加载数字人
  const avatar = new DigitalHuman(scene);
  try {
    await avatar.load("./assets/human/girl.glb");
    console.log("Avatar loaded successfully");
  } catch (e) {
    console.error("Avatar load failed:", e);
  }

  // 3. 初始化 TTS (语音合成)
  // 当 TTS 开始播放时，数字人开始张嘴；结束时闭嘴
  const tts = new TextToSpeech(
    () => avatar.setSpeaking(true),
    () => avatar.setSpeaking(false)
  );

  function animate() {
    requestAnimationFrame(animate);
    avatar.update(); // 执行呼吸、起伏、眨眼等动画
    controls.update(); // 更新控制器
    renderer.render(scene, camera);
  }
  animate();

  // 3. 加载模型逻辑 (保持 fetchBytes 逻辑不变)
  const modelPath = "./assets/model/Qwen2.5-0.5B-Instruct/";
  // const modelPath = "./assets/model/Qwen1.5-0.5B-Chat/";
  const [weights, tokenizer, config] = await Promise.all([
    fetchBytes(`${modelPath}model.safetensors`),
    fetchBytes(`${modelPath}tokenizer.json`),
    fetchBytes(`${modelPath}config.json`),
  ]);

  const engine = await LLMEngine.init(weights, tokenizer, config);

  // UI 切换逻辑...
  document.getElementById("loading-screen").style.display = "none";
  document.getElementById("ui-layer").classList.remove("hidden");

  // 4. UI 辅助函数
  function appendMessage(role, text) {
    const chatHistory = document.getElementById("chat-history");
    const msgDiv = document.createElement("div");
    msgDiv.className = `message ${role}-message`;

    const contentDiv = document.createElement("div");
    contentDiv.className = "message-content";
    contentDiv.innerText = text;

    msgDiv.appendChild(contentDiv);
    chatHistory.appendChild(msgDiv);

    // 自动滚动到底部
    chatHistory.scrollTop = chatHistory.scrollHeight;

    return contentDiv; // 返回内容容器以便后续流式更新
  }

  // 5. 发送逻辑
  const inputField = document.getElementById("userInput");
  const sendBtn = document.getElementById("sendBtn");

  async function handleSend() {
    const text = inputField.value.trim();
    if (!text) return;

    // 1. **立刻**显示用户消息并清空输入框
    appendMessage("user", text);
    inputField.value = "";

    // 给浏览器一点时间渲染 UI
    await new Promise((r) => setTimeout(r, 0));

    // 2. 构建 Prompt
    const prompt = `<|im_start|>user\n${text}<|im_end|>\n<|im_start|>assistant\n`;

    try {
      console.log("Starting streaming inference...");

      // 3. 初始化并准备推理
      engine.apply_prompt(prompt);

      // 4. 创建一个空的 AI 回复框
      const aiContent = appendMessage("ai", "");
      let fullResponse = "";

      // 停止上一次可能的语音
      tts.stop();

      // 开始说话动画
      avatar.setSpeaking(true);

      // 5. 流式推理循环：动态追踪 AI 是否说完
      while (!engine.is_finished()) {
        const piece = engine.step();

        if (piece) {
          fullResponse += piece;
          aiContent.innerText = fullResponse;

          tts.append(piece); // 喂给 TTS 进行断句和播放

          // 自动滚动
          document.getElementById("chat-history").scrollTop =
            document.getElementById("chat-history").scrollHeight;
        }

        // **关键**：微调延迟 (30ms)，配合极速 TTS
        await new Promise((r) => setTimeout(r, 30));

        // 可选：添加一个极大的安全上限防止死循环（例如 1000 tokens）
        if (fullResponse.length > 2000) break;
      }

      // 确保最后剩余文本被播放
      tts.flush();
      console.log("Stream finished.");
    } catch (e) {
      console.error("Inference Error:", e);
      appendMessage("ai", "抱歉，出错了: " + e.message);
    }
  }

  sendBtn.onclick = handleSend;
  inputField.onkeypress = (e) => {
    if (e.key === "Enter") handleSend();
  };
}

async function fetchBytes(url) {
  const res = await fetch(url);
  return new Uint8Array(await res.arrayBuffer());
}

start();
