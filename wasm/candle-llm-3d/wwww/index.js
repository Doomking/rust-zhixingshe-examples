import * as THREE from "three";
import WebGPURenderer from "three/examples/jsm/renderers/webgpu/WebGPURenderer.js";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { DigitalHuman } from "./digital_human.js";
import { TextToSpeech } from "./tts.js";

async function start() {
  //  初始化场景
  const scene = new THREE.Scene();

  //  动态绘制背景 (Procedural Background)
  function createProceduralBackground() {
    const canvas = document.createElement("canvas");
    canvas.width = 1024;
    canvas.height = 1024;
    const ctx = canvas.getContext("2d");

    // 高饱和度的蓝天、洁白的云朵、绿意盎然的草地
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
  camera.position.set(0, 1.0, 0.65);

  // 2. 增强光照
  const directionalLight = new THREE.DirectionalLight(0xffffff, 2.0);
  directionalLight.position.set(2, 4, 5);
  scene.add(directionalLight);

  // 4. 初始化渲染器 (优先尝试 WebGPU)
  let renderer;
  try {
    renderer = new WebGPURenderer({ antialias: true, alpha: true });
    console.log("[Renderer] Using WebGPU Engine");
  } catch (e) {
    console.warn("[Renderer] WebGPU not supported, falling back to WebGL:", e);
    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  }

  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.domElement.style.position = "absolute";
  renderer.domElement.style.top = "0";
  document.getElementById("canvas-container").appendChild(renderer.domElement);

  // 3. 添加轨道控制器 (OrbitControls)
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.target.set(0, 0.85, 0); // 必须以人为中心
  controls.enablePan = false;
  controls.minDistance = 0.3;
  controls.maxDistance = 3.0;

  function updateCameraOffset() {
    const w = window.innerWidth;
    const h = window.innerHeight;

    const offsetX = w * 0.25; // 偏移 25% 宽度
    camera.view = null; // 重置
    camera.setViewOffset(w, h, offsetX, 0, w, h);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();

    renderer.setSize(w, h);
  }

  updateCameraOffset();

  // 响应式调整
  window.addEventListener("resize", () => {
    updateCameraOffset();
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
    () => avatar.setSpeaking(false),
  );

  function animate() {
    requestAnimationFrame(animate);
    avatar.update(); // 执行呼吸、起伏、眨眼等动画
    controls.update(); // 更新控制器
    renderer.render(scene, camera);
  }
  animate();

  // 3. 初始化 Worker
  const worker = new Worker("./worker.js", { type: "module" });

  // 状态追踪
  let currentAiContent = null;
  let currentFullResponse = "";

  worker.onmessage = (e) => {
    const { action, token, error } = e.data;

    switch (action) {
      case "loaded":
        console.log("Worker loaded model successfully");
        document.getElementById("loading-screen").style.display = "none";
        document.getElementById("ui-layer").classList.remove("hidden");
        break;
      case "token":
        if (currentAiContent) {
          currentFullResponse += token;
          currentAiContent.innerText = currentFullResponse;
          tts.append(token);
          document.getElementById("chat-history").scrollTop =
            document.getElementById("chat-history").scrollHeight;
        }
        break;
      case "done":
        console.log("Worker finished generation");
        tts.flush();
        avatar.setSpeaking(false); // 确保停止
        break;
      case "error":
        console.error("Worker error:", error);
        if (currentAiContent) {
          currentAiContent.innerText += "\n[Error: " + error + "]";
        }
        avatar.setSpeaking(false);
        break;
    }
  };

  // 4. 加载模型逻辑 (保持 fetchBytes 逻辑不变)
  const modelPath = "./assets/model/Qwen2.5-0.5B-Instruct/";
  // const modelPath = "./assets/model/Qwen1.5-0.5B-Chat/";

  // 并行下载权重
  Promise.all([
    fetchBytes(`${modelPath}model.safetensors`),
    fetchBytes(`${modelPath}tokenizer.json`),
    fetchBytes(`${modelPath}config.json`),
  ])
    .then(([weights, tokenizer, config]) => {
      console.log("Assets downloaded, initializing worker...");
      // 发送给 Worker 初始化 (转移所有权以提高性能)
      worker.postMessage(
        {
          action: "init",
          payload: { weights, tokenizer, config },
        },
        [weights.buffer, tokenizer.buffer, config.buffer],
      );
    })
    .catch((e) => {
      console.error("Failed to download assets:", e);
      appendMessage("ai", "加载失败: " + e.message);
    });

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
      console.log("Sending prompt to worker...");

      // 3. 创建 UI
      currentAiContent = appendMessage("ai", "");
      currentFullResponse = "";

      // 4. 停止上一次可能的语音
      tts.stop();

      // 5. 开始说话动画 (Worker 回传 done 时停止)
      avatar.setSpeaking(true);

      // 6. 发送给 Worker
      worker.postMessage({ action: "generate", payload: { prompt } });
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
