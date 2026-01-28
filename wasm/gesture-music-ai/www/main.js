import init, { GestureMusicEngine } from "../pkg/gesture_music_ai.js";
import * as THREE from "three"; // 引入 Three.js

let engine, audioCtx, captureCtx;
let wasmInstance; // 添加这个全局变量
// --- Three.js 变量 ---
let scene, camera, renderer;
let particleGeometry, particleMaterial, particles; // 用于渲染流体粒子
let osc, gainNode;
async function start() {
  // 1. 初始化 WASM
  wasmInstance = await init();

  // 2. 加载 AI 模型权重 (需确保文件在相应路径)
  // 并行加载视觉和文本权重
  const modelBase = "./model/";
  const [visionWeights, textWeights, textTokenizer, textConfig] =
    await Promise.all([
      fetchBinary(
        `${modelBase}hand_landmarks/hand_landmarks_detector.safetensors`,
      ),
      // fetchBinary(`${modelBase}Qwen1.5-0.5B-Chat/model.safetensors`),
      // fetchBinary(`${modelBase}Qwen1.5-0.5B-Chat/tokenizer.json`),
      // fetchBinary(`${modelBase}Qwen1.5-0.5B-Chat/config.json`),
    ]);
  // 实例化：传入所有权据
  engine = new GestureMusicEngine(
    new Uint8Array(visionWeights),
    new Uint8Array(textWeights),
    new Uint8Array(textTokenizer),
    new Uint8Array(textConfig),
  );
  document.getElementById("ai-status").innerText = "READY";

  // 4. 初始化音频上下文 (需用户交互触发)
  setupAudio();

  // 5. 开启摄像头
  const video = document.getElementById("webcam");
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: 224, height: 224 },
  });
  video.srcObject = stream;

  // 核心修复：确保视频可以播放
  await video.play();

  // 准备采集 Canvas
  const captureCanvas = document.getElementById("capture-canvas");
  captureCtx = captureCanvas.getContext("2d", { willReadFrequently: true });

  // --- 初始化 Three.js 场景 ---
  initThreeJS();

  // 启动主循环
  requestAnimationFrame(gameLoop);
  document.getElementById("start-hint").style.display = "none";
}

function initThreeJS() {
  scene = new THREE.Scene();
  camera = new THREE.PerspectiveCamera(
    75,
    window.innerWidth / window.innerHeight,
    0.1,
    1000,
  );
  renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.setClearColor(0x000000, 1); // 设置背景色为黑色
  document.body.appendChild(renderer.domElement); // 将 Three.js 的 Canvas 添加到 body

  camera.position.z = 5; // 调整摄像头位置

  // 创建粒子系统：Geometry 存放位置，Material 定义外观
  particleGeometry = new THREE.BufferGeometry();
  // 最多 500 个粒子 * (x, y, z) + (r, g, b, a) = 500 * (3 + 4) = 3500 个浮点数
  // 我们会根据 Rust 返回的 fluidData 来填充这个 Buffer
  const positions = new Float32Array(500 * 3); // x, y, z
  const colors = new Float32Array(500 * 4); // r, g, b, a (生命周期映射到透明度)

  particleGeometry.setAttribute(
    "position",
    new THREE.BufferAttribute(positions, 3),
  );
  particleGeometry.setAttribute("color", new THREE.BufferAttribute(colors, 4));

  particleMaterial = new THREE.PointsMaterial({
    size: 0.1, // 粒子大小
    vertexColors: true, // 允许每个粒子有独立颜色
    blending: THREE.AdditiveBlending, // 叠加混合，让粒子更亮
    transparent: true, // 允许透明度
    sizeAttenuation: true, // 粒子大小随距离变化
  });

  particles = new THREE.Points(particleGeometry, particleMaterial);
  scene.add(particles);

  // 窗口大小变化时更新渲染器
  window.addEventListener("resize", () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
  });
}

let lastPoemUpdateTime = 0;

let count = 0;

async function gameLoop() {
  if (!engine || !wasmInstance) {
    // 检查 instance 是否准备好
    requestAnimationFrame(gameLoop);
    return;
  }
  // if (count >= 4) {
  //   return;
  // }
  count++;
  // 1. 确保视频已准备好，避免传入空数据导致 Rust Panic
  const video = document.getElementById("webcam");
  if (video.readyState >= 2) {
    captureCtx.drawImage(video, 0, 0, 224, 224);
    const imageData = captureCtx.getImageData(0, 0, 224, 224);

    // console.log(imageData.data);
    // console.log("imageData.data");
    try {
      // 2. 执行核心逻辑
      const pixels = imageData.data; // Uint8ClampedArray

      // console.log("JS Sample:", pixels[0], pixels[1], pixels[2]);
      // 1. 在 WASM 内存中分配空间 (通常使用 wasm-bindgen 自动生成的 malloc)
      // 或者如果你在 engine 里预留了缓冲区，直接获取那个指针
      const numBytes = pixels.length;
      const ptr = wasmInstance.__wbindgen_malloc(numBytes, 1); // 申请内存

      // 2. 创建一个指向 WASM 内存的视图并写入数据
      const wasmMemory = new Uint8Array(wasmInstance.memory.buffer);
      wasmMemory.set(pixels, ptr);

      try {
        // 3. 将指针传给 Rust
        const result = engine.tick(ptr, 224, 224);
        if (result) {
          // 解构从 Rust 返回的数据 (audio_frame, visual_data, gesture_id)
          const [_, fluidData, gestureId, confidence] = result;
          console.log("Gesture ID:", gestureId);
          console.log("fluidData", fluidData);

          console.log(
            `Gesture: ${gestureId}, Confidence: ${confidence.toFixed(4)}`,
          );

          // 2. 传入真实的置信度
          // 注意：如果置信度太低（比如 < 0.01），可以考虑静音，避免底噪干扰
          if (confidence > 0.005) {
            playSynthesizedSound(gestureId, confidence);
          } else {
            // 如果 AI 什么都没看到，让声音逐渐消失
            // stopSound();
          }
          updateFluidParticles(fluidData);
        }
      } finally {
        // 4. 重要：推理结束后释放内存，防止内存泄漏
        wasmInstance.__wbindgen_free(ptr, numBytes, 1);
      }

      // 3. 在同一个循环内，根据频率获取诗意文字
      // 这样保证了 get_poetic_text 永远不会与 tick 同时运行
      const now = Date.now();
      if (now - lastPoemUpdateTime > 3000) {
        // 每3秒更新一次
        const poem = engine.get_poetic_text();
        updatePoemDisplay(poem);
        lastPoemUpdateTime = now;
      }
    } catch (e) {
      console.error("Engine processing error:", e);
    }
  }

  renderer.render(scene, camera);
  requestAnimationFrame(gameLoop);
}

function playBuffer(samples) {
  if (!samples || samples.length === 0) return;
  const buffer = audioCtx.createBuffer(1, samples.length, 44100);
  buffer.getChannelData(0).set(samples);
  const source = audioCtx.createBufferSource();
  source.buffer = buffer;
  source.connect(audioCtx.destination);
  source.start();
}

// === 新增 Three.js 粒子更新函数 ===
let currentParticleIdx = 0;
const MAX_PARTICLES = 500;

let trailPoints = []; // 记录手势轨迹的队列

function updateFluidParticles(points) {
  const positions = particleGeometry.attributes.position.array;
  const colors = particleGeometry.attributes.color.array;
  const aspect = window.innerWidth / window.innerHeight;

  // 1. 从 Rust 获取新点 (假设 points 为 [x, y, confidence])
  const [nx, ny, conf] = points;
  if (conf > 0.2) {
    // 将新点加入队列头部
    trailPoints.unshift({
      x: (nx - 0.5) * aspect * 15, // 放大范围
      y: -(ny - 0.5) * 15,
      age: 1.0,
    });
  }

  // 2. 限制轨迹长度
  if (trailPoints.length > 500) trailPoints.pop();

  // 3. 更新所有粒子的状态
  for (let i = 0; i < 500; i++) {
    const p = trailPoints[i];
    if (p) {
      // 让点随时间漂移，产生“流体”感
      p.x += Math.sin(Date.now() * 0.001 + i * 0.1) * 0.01;
      p.y -= 0.02; // 缓慢下沉
      p.age *= 0.98; // 逐渐淡出

      positions[i * 3 + 0] = p.x;
      positions[i * 3 + 1] = p.y;
      positions[i * 3 + 2] = 0;

      // 颜色：从青色变为紫色
      colors[i * 4 + 0] = 1.0 - p.age; // R
      colors[i * 4 + 1] = p.age; // G
      colors[i * 4 + 2] = 1.0; // B
      colors[i * 4 + 3] = p.age; // Alpha
    } else {
      colors[i * 4 + 3] = 0; // 隐藏多余粒子
    }
  }

  particleGeometry.attributes.position.needsUpdate = true;
  particleGeometry.attributes.color.needsUpdate = true;
}

document.getElementById("start-btn").onclick = start;

let textElement = document.createElement("div");
textElement.id = "poetic-text";
document.body.appendChild(textElement);

// 定时获取诗意文字
// setInterval(() => {
//   if (engine) {
//     const poem = engine.get_poetic_text();
//     updatePoemDisplay(poem);
//   }
// }, 3000);

// 辅助：更新 UI 上的文本
function updatePoemDisplay(text) {
  const el = document.getElementById("poetic-text");
  if (!el) return;
  el.style.opacity = 0;
  setTimeout(() => {
    el.innerText = text;
    el.style.opacity = 1;
  }, 500);
}

async function fetchBinary(url) {
  const response = await fetch(url);
  const buffer = await response.arrayBuffer();
  return new Uint8Array(buffer);
}

function setupAudio() {
  audioCtx = new (window.AudioContext || window.webkitAudioContext)();

  // 创建一个主振荡器
  osc = audioCtx.createOscillator();
  gainNode = audioCtx.createGain();

  osc.type = "sine"; // 正弦波最纯净，不会有 bubu 声
  osc.frequency.setValueAtTime(440, audioCtx.currentTime);

  gainNode.gain.setValueAtTime(0, audioCtx.currentTime); // 初始静音

  osc.connect(gainNode);
  gainNode.connect(audioCtx.destination);
  osc.start();
}

// 定义一个五声音阶（Pentatonic Scale），怎么弹都好听
const PENTATONIC_C4 = [261.63, 293.66, 329.63, 392.0, 440.0, 523.25];

function playSynthesizedSound(id, confidence) {
  if (!gainNode) return;
  const now = audioCtx.currentTime;

  // 将置信度映射到音量
  // 即使置信度只有 0.01，也可以通过指数映射让它变得有意义
  const volume = Math.pow(confidence, 2) * 2; // 指数映射让强信号更突出

  // 根据 ID 变换频率
  const freq = 220 * Math.pow(2, (id % 12) / 12);

  osc.frequency.setTargetAtTime(freq, now, 0.1);
  gainNode.gain.setTargetAtTime(volume, now, 0.05); // 音量随置信度实时波动
}
