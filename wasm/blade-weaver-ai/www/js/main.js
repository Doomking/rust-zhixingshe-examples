import init, { BladeCore } from "../../pkg/blade_weaver_ai.js";
import { VisionSystem } from "./vision.js";
import { GameScene } from "./scene.js";

class GameController {
  constructor() {
    this.vision = null;
    this.scene = null;
    this.rustCore = null;
    this.aiWorker = null;
    this.isGameRunning = false;

    // UI 元素
    this.startBtn = document.getElementById("start-btn");
    this.subtitle = document.getElementById("ai-subtitle");
    this.loadingOverlay = document.getElementById("loading-overlay");
  }

  async prepare() {
    console.log("🚀 正在唤醒刀锋织语者内核...");

    // 1. 并行初始化 WASM 和 视觉系统
    const [wasm] = await Promise.all([init(), this.initVision()]);

    this.rustCore = new BladeCore();
    this.scene = new GameScene(
      document.getElementById("game-canvas"),
      this.rustCore,
    );

    // 2. 初始化 AI 后台线程
    this.initAIWorker();

    // 3. 监听游戏逻辑事件
    this.setupEventListeners();

    this.loadingOverlay.style.display = "none";
    this.startBtn.disabled = false;
    this.startBtn.innerText = "进入次元裂缝";
  }

  async initVision() {
    this.vision = new VisionSystem();
    await this.vision.init();
  }

  initAIWorker() {
    this.aiWorker = new Worker(
      new URL("./ai_worker.js", import.meta.url),
      { type: "module" },
    );

    this.aiWorker.onmessage = (e) => {
      const { text, type } = e.data;
      if (text && type === 'commentary') {
        // 异步处理，避免阻塞游戏线程
        requestAnimationFrame(() => this.narrate(text));
      }
    };
  }

  setupEventListeners() {
    this.startBtn.addEventListener("click", () => this.startGame());

    // 监听来自 scene.js 的切割事件
    window.addEventListener("fruit-sliced", (e) => {
      // 将切割事件同步给 AI 导演
      if (Date.now() - this.lastAiTime > 5000) {
this.aiWorker.postMessage({
        type: "game_event",
        event: { action: "slice", item: e.detail.type, time: Date.now() },
      });
        this.lastAiTime = Date.now();
    }
      
    });
  }

  startGame() {
    this.isGameRunning = true;
    this.gameStartTime = Date.now();
    this.startBtn.style.display = "none";

    // 发送启动指令给 AI
    this.aiWorker.postMessage({
      type: "game_event",
      event: { action: "game_start" },
    });

    // 启动主游戏循环
    this.gameLoop();

    // 启动水果生成调度
    this.scheduleFruits();
  }

  // AI 语音合成与字幕展示
  narrate(text) {
    this.subtitle.innerText = text;
    this.subtitle.classList.add("active");

    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = "zh-CN";
    utterance.pitch = 1.2;
    utterance.rate = 1.1;

    utterance.onend = () => {
      setTimeout(() => this.subtitle.classList.remove("active"), 2000);
    };

    window.speechSynthesis.cancel(); // 停止之前的播报，确保实时性
    window.speechSynthesis.speak(utterance);
  }

  scheduleFruits() {
    if (!this.isGameRunning) return;
    
    // 检查场景中的活跃水果数量
    const activeFruits = this.scene.getActiveFruitCount();
    const MAX_FRUITS = 50;
    
    // 游戏时间（用于动态难度调整）
    const gameTime = (Date.now() - this.gameStartTime) / 1000;
    
    // 基础难度系数（随时间增加）
    const difficultyFactor = Math.min(1 + gameTime / 60, 2); // 最高2倍难度
    
    // 根据当前负载和难度动态调整生成频率和数量
    let spawnCount = 0;
    let nextSpawnInterval = 1500; // 默认间隔
    
    if (activeFruits < MAX_FRUITS * 0.4) {
      // 水果少于40%上限：快速生成
      spawnCount = Math.floor(Math.random() * 3) + 2; // 2-4个
      nextSpawnInterval = Math.max(800, 1500 / difficultyFactor);
    } else if (activeFruits < MAX_FRUITS * 0.7) {
      // 水果在40-70%之间：正常生成
      spawnCount = Math.floor(Math.random() * 3) + 1; // 1-3个
      nextSpawnInterval = Math.max(1000, 1500 / difficultyFactor);
    } else if (activeFruits < MAX_FRUITS) {
      // 水果在70-100%之间：减少生成
      spawnCount = Math.floor(Math.random() * 2) + 1; // 1-2个
      nextSpawnInterval = Math.max(1200, 2000 / difficultyFactor);
    } else {
      // 已达到上限：暂停生成
      spawnCount = 0;
      nextSpawnInterval = 800; // 快速重新检查
    }
    
    // 添加调试信息输出
    if (typeof process !== 'undefined' && process.env?.NODE_ENV !== 'production') {
      console.log(`🍎 水果生成: ${spawnCount}个, 活跃水果: ${activeFruits}/${MAX_FRUITS}, 下次间隔: ${nextSpawnInterval}ms, 难度: ${difficultyFactor.toFixed(2)}`);
    }
    
    // 执行水果生成
    for (let i = 0; i < spawnCount; i++) {
      this.scene.spawnFruit();
    }
    
    // 调整生成间隔
    setTimeout(() => this.scheduleFruits(), nextSpawnInterval);
  }

  gameLoop() {
    if (!this.isGameRunning) return;

    const rawPos = this.vision.detect();

    if (rawPos) {
      try {
        // 只调用一次 rustCore.update_hand() 进行平滑处理
        const smoothed = this.rustCore.update_hand(rawPos.x, rawPos.y);

        // 只在非生产环境打印调试信息
        if (typeof process !== 'undefined' && process.env?.NODE_ENV !== 'production') {
          console.log("Rust 过滤后的坐标:", smoothed);
        }

        // 将平滑后的坐标转换为对象格式传给 scene.update()
        this.scene.update({ x: smoothed[0], y: smoothed[1] });
      } catch (error) {
        console.error("❌ 手势处理错误:", error);
      }
    } else {
      // 处理视觉检测失败的情况
      if (typeof process !== 'undefined' && process.env?.NODE_ENV !== 'production') {
        console.warn("⚠️ 视觉检测失败，跳过本帧更新");
      }
      this.scene.update(null);
    }

    requestAnimationFrame(() => this.gameLoop());
  }
}

// 启动应用
const app = new GameController();
app.prepare();
