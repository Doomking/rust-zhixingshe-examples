// 所有的 WASM 逻辑已移入 AI 和 Physics Worker
// --- 0. 配置：只需给个长度和后缀 ---
const IMAGE_COUNT = 10; // 运行完 Python 脚本后修改这里
const IMAGE_EXT = ".jpg";
const ASSETS_PATH = "./assets/images/";

let aiWorker, physWorker;
let gl, program, particleBuffer;
let audioCtx, analyser, synth;
let isSpacePressed = false;
let currentParticles = new Float32Array(0);
let isPhysBusy = false;

let mouseX = 0;
let mouseY = 0;

// 存储当前图片的字节和原始像素，供注入使用
let currentImageBytes = null;
let currentImageData = null;
let currentImageDims = { w: 0, h: 0 };

// --- 1. WebGL 着色器源代码 ---
const vs = `
    attribute vec2 a_position;
    attribute vec3 a_color;
    attribute float a_life; // 新增：生命周期
    uniform vec2 u_resolution;
    varying vec3 v_color;
    varying float v_life;
    void main() {
        vec2 zeroToOne = a_position / u_resolution;
        vec2 zeroToTwo = zeroToOne * 2.0;
        vec2 clipSpace = zeroToTwo - 1.0;

        // 大幅增加粒子尺寸方案
        // 基础大小 2.0，生命值加成从 4.0 到 12.0
        gl_PointSize = (2.0 + a_life * 12.0);
        gl_Position = vec4(clipSpace * vec2(1, -1), 0.0, 1.0);
        v_color = a_color;
        v_life = a_life;
    }
`;

const fs = `
    precision mediump float;
    varying vec3 v_color;
    varying float v_life;
    void main() {
        float dist = distance(gl_PointCoord, vec2(0.5));
        if (dist > 0.5) discard;

        // 极致视觉：脉冲边缘 + 核心光辉
        float core = (1.0 - smoothstep(0.0, 0.2, dist));
        float glow = (1.0 - smoothstep(0.0, 0.5, dist));

        // 随生命周期变化的 shimmering
        float pulse = 1.0 + 0.2 * sin(v_life * 30.0);
        float alpha = glow * v_life * 0.8;

        // 最终颜色强化
        vec3 finalColor = v_color * pulse * 1.8;
        gl_FragColor = vec4(finalColor, alpha);
    }
`;

// --- 2. 初始化引擎 ---
async function start() {
  const loading = document.getElementById("loading-overlay");
  loading.style.display = "flex";

  // 1. 初始化两个 Worker
  aiWorker = new Worker("ai-worker.js", { type: "module" });
  physWorker = new Worker("phys-worker.js", { type: "module" });

  let aiReady = false;
  let physReady = false;

  const checkReady = () => {
    if (aiReady && physReady) {
      console.log("Main: Both Workers READY");
      loading.style.display = "none";
      setupAudio();
      renderGallery();
      setupUI();
      requestAnimationFrame(loop);
    }
  };

  aiWorker.onmessage = (e) => {
    const { type, mask, bounds, error } = e.data;
    if (type === "READY") {
      aiReady = true;
      checkReady();
    } else if (type === "IMAGE_READY") {
      loading.style.display = "none";
      const statusText = document.getElementById("loader-text");
      if (statusText) statusText.innerText = "SYSTEM READY";
    } else if (type === "MASK_READY") {
      const { mask, material, bounds, active_pixels } = e.data;

      if (!material) {
        console.warn("Main: Received MASK_READY but material is undefined.");
        return;
      }

      // 1. 更新材质 UI 和音效
      const label = document.getElementById("material-label");
      if (label)
        label.innerText = `MATERIAL: ${material.label?.toUpperCase() || "UNKNOWN"}`;
      if (material.label) playMaterialSound(material, active_pixels);

      // 2. 更新 Phys Worker 物理参数
      physWorker.postMessage({
        type: "UPDATE_PARAMS",
        data: {
          viscosity: material.viscosity || 0.1,
          density: material.density || 1.0,
        },
      });

      // 3. 注入粒子
      physWorker.postMessage(
        {
          type: "INJECT",
          data: {
            mask,
            imgW: currentImageDims.w,
            imgH: currentImageDims.h,
            offset_x: bounds.left,
            offset_y: bounds.top,
            display_w: bounds.width,
            display_h: bounds.height,
            scaled_w: e.data.scaled_w,
            scaled_h: e.data.scaled_h,
            material,
          },
        },
        [mask.buffer],
      );
    } else if (type === "ERROR") {
      console.error("AIWorker Error:", error);
      alert("AI Error: " + error);
    }
  };

  physWorker.onmessage = (e) => {
    const { type, particles, error } = e.data;
    if (type === "READY") {
      physReady = true;
      checkReady();
    } else if (type === "TICK") {
      currentParticles = particles;
      isPhysBusy = false;
    } else if (type === "ERROR") {
      console.error("PhysWorker Error:", error);
    }
  };

  // 2. 初始化本地 WebGL (支持 DPR)
  const canvas = document.getElementById("fluid-canvas");
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;

  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;

  gl = canvas.getContext("webgl", { alpha: true });
  gl.viewport(0, 0, canvas.width, canvas.height);

  const vShader = createShader(gl, gl.VERTEX_SHADER, vs);
  const fShader = createShader(gl, gl.FRAGMENT_SHADER, fs);
  program = createProgram(gl, vShader, fShader);

  gl.enable(gl.BLEND);
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE);

  // 3. 准备模型数据并传给 AI Worker
  const modelResp = await fetch("../model/sam/mobile_sam.safetensors");
  const modelData = new Uint8Array(await modelResp.arrayBuffer());

  aiWorker.postMessage({
    type: "INIT",
    data: { modelData },
  });

  physWorker.postMessage({
    type: "INIT",
    data: { width: canvas.width, height: canvas.height },
  });

  // 4. 处理窗口 Resize
  window.addEventListener("resize", () => {
    const newRect = canvas.getBoundingClientRect();
    canvas.width = newRect.width * dpr;
    canvas.height = newRect.height * dpr;
    gl.viewport(0, 0, canvas.width, canvas.height);
    physWorker.postMessage({
      type: "RESIZE",
      data: { width: canvas.width, height: canvas.height },
    });
  });
}

// initGL function is removed as its responsibilities are split between start() and the worker.

function setupAudio() {
  audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  analyser = audioCtx.createAnalyser();
  analyser.fftSize = 256;
  synth = new MaterialSynth(audioCtx);
}

class MaterialSynth {
  constructor(ctx) {
    this.ctx = ctx;
    this.masterGain = ctx.createGain();
    this.masterGain.gain.value = 0.4;
    this.masterGain.connect(ctx.destination);

    // 简单混响 (Delay 模拟)
    this.delay = ctx.createDelay();
    this.delay.delayTime.value = 0.15;
    this.delayFeedback = ctx.createGain();
    this.delayFeedback.gain.value = 0.25;

    this.delay.connect(this.delayFeedback);
    this.delayFeedback.connect(this.delay);
    this.delay.connect(this.masterGain);

    // 连接 Analyser 用于可视化
    // masterGain -> Analyser -> Destination
    if (analyser) {
      this.masterGain.connect(analyser);
      analyser.connect(ctx.destination);
    } else {
      this.masterGain.connect(ctx.destination);
    }

    this.masterGain.connect(this.delay);
  }

  play(material, activePixels = 1000) {
    const now = this.ctx.currentTime;
    const sizeFactor = Math.min(activePixels / 8000, 1.0); // 空间量级 [0, 1]

    // 物体越大，延迟越深，反馈越强
    this.delay.delayTime.linearRampToValueAtTime(0.1 + sizeFactor * 0.4, now);
    this.delay.feedback?.linearRampToValueAtTime(0.2 + sizeFactor * 0.3, now);

    const filter = this.ctx.createBiquadFilter();
    const env = this.ctx.createGain();

    // 基础频率由色相动态决定
    const baseFreq = material.baseFrequency || 200 + (material.hue || 0);

    filter.type = "lowpass";
    filter.frequency.setValueAtTime(2000, now);
    filter.Q.setValueAtTime(1, now);

    if (material.label.includes("Light")) {
      const osc1 = this.ctx.createOscillator();
      const osc2 = this.ctx.createOscillator();
      osc1.frequency.setValueAtTime(baseFreq, now);
      osc2.frequency.setValueAtTime(baseFreq * 2.01, now);
      osc1.type = "sine";
      osc2.type = "sine";
      filter.type = "highpass";
      filter.frequency.setValueAtTime(2000, now);
      env.gain.setValueAtTime(0, now);
      env.gain.linearRampToValueAtTime(0.5, now + 0.01);
      env.gain.exponentialRampToValueAtTime(0.001, now + 2.0);
      osc1.connect(filter);
      osc2.connect(filter);
      osc1.start(now);
      osc2.start(now);
      osc1.stop(now + 2.1);
      osc2.stop(now + 2.1);
    } else if (
      material.label.includes("Void") ||
      material.label.includes("Black")
    ) {
      const osc = this.ctx.createOscillator();
      osc.type = "sine";
      osc.frequency.setValueAtTime(baseFreq, now);
      osc.frequency.exponentialRampToValueAtTime(baseFreq * 0.5, now + 1.0);
      filter.frequency.setValueAtTime(200, now);
      env.gain.setValueAtTime(0, now);
      env.gain.linearRampToValueAtTime(0.8, now + 0.2);
      env.gain.exponentialRampToValueAtTime(0.001, now + 3.0);
      osc.connect(filter);
      osc.start(now);
      osc.stop(now + 3.1);
    } else if (
      material.label.includes("Red") ||
      material.label.includes("Warm")
    ) {
      const carrier = this.ctx.createOscillator();
      const modulator = this.ctx.createOscillator();
      const modGain = this.ctx.createGain();
      carrier.frequency.setValueAtTime(baseFreq, now);
      modulator.frequency.setValueAtTime(baseFreq * 0.5, now);
      modGain.gain.setValueAtTime(100, now);
      modulator.connect(modGain);
      modGain.connect(carrier.frequency);
      env.gain.setValueAtTime(0, now);
      env.gain.linearRampToValueAtTime(0.4, now + 0.05);
      env.gain.exponentialRampToValueAtTime(0.001, now + 1.0);
      carrier.connect(filter);
      carrier.start(now);
      modulator.start(now);
      carrier.stop(now + 1.1);
      modulator.stop(now + 1.1);
    } else if (
      material.label.includes("Blue") ||
      material.label.includes("Water")
    ) {
      const osc = this.ctx.createOscillator();
      osc.type = "sine";
      osc.frequency.setValueAtTime(baseFreq * 2, now);
      osc.frequency.exponentialRampToValueAtTime(baseFreq, now + 0.5);
      filter.type = "bandpass";
      filter.frequency.setValueAtTime(baseFreq, now);
      filter.Q.setValueAtTime(20, now);
      env.gain.setValueAtTime(0, now);
      env.gain.linearRampToValueAtTime(0.6, now + 0.01);
      env.gain.exponentialRampToValueAtTime(0.001, now + 1.5);
      osc.connect(filter);
      osc.start(now);
      osc.stop(now + 1.6);
    } else {
      const osc = this.ctx.createOscillator();
      osc.type = "triangle";
      osc.frequency.setValueAtTime(baseFreq, now);
      env.gain.setValueAtTime(0, now);
      env.gain.linearRampToValueAtTime(0.3, now + 0.1);
      env.gain.exponentialRampToValueAtTime(0.001, now + 1.0);
      osc.connect(filter);
      osc.start(now);
      osc.stop(now + 1.1);
    }
    filter.connect(env);
    env.connect(this.masterGain);
  }
}

async function handleImageChange(imgUrl) {
  const loading = document.getElementById("loading-overlay");
  loading.style.display = "flex";
  const statusText = document.getElementById("loader-text");
  if (statusText) statusText.innerText = "DECONSTRUCTING...";

  const sourceImage = document.getElementById("source-image");
  if (sourceImage) {
    sourceImage.src = imgUrl;
    // 获取原始像素数据供 Physics Worker 使用
    const img = new Image();
    img.src = imgUrl;
    await new Promise((resolve) => (img.onload = resolve));

    const tempCanvas = document.createElement("canvas");
    tempCanvas.width = img.width;
    tempCanvas.height = img.height;
    const ctx = tempCanvas.getContext("2d");
    ctx.drawImage(img, 0, 0);
    currentImageData = ctx.getImageData(0, 0, img.width, img.height).data;
    currentImageDims = { w: img.width, h: img.height };
  }

  const resp = await fetch(imgUrl);
  const buffer = await resp.arrayBuffer();
  currentImageBytes = new Uint8Array(buffer);

  aiWorker.postMessage(
    {
      type: "SET_IMAGE",
      data: { imageBytes: buffer },
    },
    [buffer],
  );
}

function setupUI() {
  // 使用事件委托或者在生成后绑定
  document.querySelector(".gallery")?.addEventListener("click", async (e) => {
    if (e.target.classList.contains("thumb-img")) {
      await handleImageChange(e.target.src);
    }
  });

  // 交互逻辑
  const wrapper = document.getElementById("canvas-wrapper");
  wrapper.addEventListener("mousedown", handleCanvasClick);
  wrapper.addEventListener("mousemove", (e) => {
    const rect = wrapper.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    mouseX = (e.clientX - rect.left) * dpr;
    mouseY = (e.clientY - rect.top) * dpr;
  });

  // 键盘监听：空格键触发塌陷
  window.addEventListener("keydown", (e) => {
    if (e.code === "Space") {
      isSpacePressed = true;
      e.preventDefault();
      const tag = document.getElementById("status-text");
      if (tag) tag.innerText = "COLLAPSING...";
    }
  });

  window.addEventListener("keyup", (e) => {
    if (e.code === "Space") {
      isSpacePressed = false;
      const tag = document.getElementById("status-text");
      if (tag) tag.innerText = "SYSTEM READY";
    }
  });
}

function handleCanvasClick(e) {
  if (audioCtx.state === "suspended") audioCtx.resume();

  const img = document.getElementById("source-image");
  const canvas = document.getElementById("fluid-canvas");

  // 必须使用物理像素 Canvas 尺寸作为绝对参考
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();

  const canvasW = canvas.width;
  const canvasH = canvas.height;

  const nW = img.naturalWidth;
  const nH = img.naturalHeight;
  if (!nW || !nH) {
    console.warn("[Interaction] Image not fully loaded, using fallback ratio");
  }

  const bounds = getImageBounds(img, canvasW, canvasH);

  // 计算点击位置 (转换为物理像素)
  const clickX = (e.clientX - rect.left) * dpr;
  const clickY = (e.clientY - rect.top) * dpr;

  if (
    clickX < bounds.left ||
    clickX > bounds.right ||
    clickY < bounds.top ||
    clickY > bounds.bottom
  ) {
    return;
  }

  const xNorm = (clickX - bounds.left) / bounds.width;
  const yNorm = (clickY - bounds.top) / bounds.height;

  aiWorker.postMessage({
    type: "INTERACT",
    data: { x: xNorm, y: yNorm, bounds },
  });
}

/**
 * 计算 object-fit: contain 下图片的实际显示边界
 */
function getImageBounds(img, containerW, containerH) {
  const naturalW = img.naturalWidth || 1024;
  const naturalH = img.naturalHeight || 1024;
  const imgRatio = naturalW / naturalH;
  const containerRatio = containerW / containerH;

  let displayW, displayH;
  if (imgRatio > containerRatio) {
    // 宽度撑满，上下有黑边
    displayW = containerW;
    displayH = containerW / imgRatio;
  } else {
    // 高度撑满，左右有黑边
    displayH = containerH;
    displayW = containerH * imgRatio;
  }

  const left = (containerW - displayW) / 2;
  const top = (containerH - displayH) / 2;

  return {
    left,
    top,
    right: left + displayW,
    bottom: top + displayH,
    width: displayW,
    height: displayH,
  };
}

// --- 3. 核心循环 ---
function loop() {
  const byteData = new Uint8Array(analyser.frequencyBinCount);
  analyser.getByteFrequencyData(byteData);

  let sum = 0;
  for (let i = 0; i < byteData.length; i++) sum += byteData[i];
  const avgAudio = sum / byteData.length / 255.0;

  // 鼠标位置 (Physical Pixels)
  // 此处可以使用简单的全局变量记录最新的鼠标位置，为简单起见先取中心
  // 改进：可以在 handleMouseMove 中记录
  const mouse_x = 0;
  const mouse_y = 0;

  if (!isPhysBusy && physWorker) {
    isPhysBusy = true;
    physWorker.postMessage({
      type: "RENDER",
      data: { avgAudio, mouse_x: mouseX, mouse_y: mouseY },
    });

    if (isSpacePressed) {
      physWorker.postMessage({
        type: "TRIGGER_COLLAPSE",
        data: { avgAudio },
      });
    }
  }

  updateVisualizer(byteData);
  renderParticles(currentParticles);

  requestAnimationFrame(loop);
}

function renderParticles(particles) {
  if (!particles || particles.length === 0) return;

  gl.clearColor(0, 0, 0, 0);
  gl.clear(gl.COLOR_BUFFER_BIT);

  gl.useProgram(program);

  // 更新视口和分辨率 (防止 Resize 后不匹配)
  gl.viewport(0, 0, gl.canvas.width, gl.canvas.height);
  gl.uniform2f(
    gl.getUniformLocation(program, "u_resolution"),
    gl.canvas.width,
    gl.canvas.height,
  );

  const posLoc = gl.getAttribLocation(program, "a_position");
  const colorLoc = gl.getAttribLocation(program, "a_color");
  const lifeLoc = gl.getAttribLocation(program, "a_life");

  gl.enableVertexAttribArray(posLoc);
  gl.enableVertexAttribArray(colorLoc);
  gl.enableVertexAttribArray(lifeLoc);

  if (!particleBuffer) {
    particleBuffer = gl.createBuffer();
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, particleBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, particles, gl.DYNAMIC_DRAW);

  // 每个粒子 6 个 float (x, y, r, g, b, life) -> 24 bytes
  gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 24, 0);
  gl.vertexAttribPointer(colorLoc, 3, gl.FLOAT, false, 24, 8);
  gl.vertexAttribPointer(lifeLoc, 1, gl.FLOAT, false, 24, 20);

  gl.drawArrays(gl.POINTS, 0, particles.length / 6);
}

function updateVisualizer(data) {
  const viz = document.getElementById("visualizer");
  if (!viz) return;

  // 初始化 Bars
  if (viz.children.length === 0) {
    for (let i = 0; i < 16; i++) {
      const bar = document.createElement("div");
      bar.className = "viz-bar";
      viz.appendChild(bar);
    }
  }

  // 更新高度
  const step = Math.floor(data.length / 16);
  for (let i = 0; i < 16; i++) {
    const val = data[i * step] / 255;
    viz.children[i].style.height = `${val * 100}%`;
  }
}

// --- 工具函数 ---
function createShader(gl, type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  return shader;
}

function createProgram(gl, vs, fs) {
  const p = gl.createProgram();
  gl.attachShader(p, vs);
  gl.attachShader(p, fs);
  gl.linkProgram(p);
  return p;
}

function playMaterialSound(material) {
  if (synth) synth.play(material);
  // console.log(`播放 ${material.label} 音色, 频率: ${material.baseFrequency}`);
}

// --- 1. 自动生成画廊 ---
function renderGallery() {
  const galleryContainer = document.querySelector(".gallery");
  const title = galleryContainer.querySelector("h3");
  galleryContainer.innerHTML = "";
  if (title) galleryContainer.appendChild(title);

  // 基于长度进行循环
  for (let i = 1; i <= IMAGE_COUNT; i++) {
    const fileName = `img${i}${IMAGE_EXT}`;
    const thumb = document.createElement("div");
    thumb.className = "thumb";
    thumb.dataset.src = `${ASSETS_PATH}${fileName}`;

    thumb.innerHTML = `
            <img class="thumb-img" src="${ASSETS_PATH}${fileName}" alt="Matter ${i}">
            <span>MATTER ${i}</span>
        `;

    galleryContainer.appendChild(thumb);
    if (i === 1) {
      handleImageChange(thumb.dataset.src);
    }
  }
}

start();
