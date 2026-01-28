import { HandLandmarker, FilesetResolver } from "@mediapipe/tasks-vision";

export class VisionSystem {
  constructor() {
    this.handLandmarker = null;
    this.video = null;
    this.lastVideoTime = -1;
    this.results = null;
  }

  async init() {
    // 1. 获取 WASM 运行时资源
    const vision = await FilesetResolver.forVisionTasks(
      "https://cdn.jsdelivr.net/npm/@mediapipe/tasks-vision@0.10.x/wasm",
    );

    // 2. 初始化手势识别器
    this.handLandmarker = await HandLandmarker.createFromOptions(vision, {
      baseOptions: {
        modelAssetPath: `https://storage.googleapis.com/mediapipe-models/hand_landmarker/hand_landmarker/float16/1/hand_landmarker.task`,
        delegate: "GPU",
      },
      runningMode: "VIDEO",
      numHands: 1,
    });

    // 3. 启动摄像头
    await this.setupCamera();
    
    // 4. 抑制调试输出
    if (typeof console !== "undefined") {
      const originalWarn = console.warn;
      console.warn = function(...args) {
        // 过滤 OpenGL 和 MediaPipe 内部警告
        const message = String(args[0]);
        if (!message.includes("GL") && !message.includes("NORM_RECT")) {
          originalWarn.apply(console, args);
        }
      };
    }
  }

  async setupCamera() {
    this.video = document.createElement("video");
    const constraints = { video: { width: 1280, height: 720 } };
    const stream = await navigator.mediaDevices.getUserMedia(constraints);
    this.video.srcObject = stream;
    this.video.setAttribute("autoplay", "");
    this.video.setAttribute("muted", "");
    this.video.setAttribute("playsinline", "");

    return new Promise((resolve) => {
      this.video.onloadedmetadata = () => {
        this.video.play();
        resolve();
      };
    });
  }

  detect() {
    // 增加对 video 状态的严格检查
    if (!this.handLandmarker || !this.video || this.video.readyState < 2)
      return null;

    const startTimeMs = performance.now();

    if (this.lastVideoTime !== this.video.currentTime) {
      this.lastVideoTime = this.video.currentTime;

      // 确保视频尺寸已正确加载，并使用标准化坐标
      if (this.video.videoWidth > 0 && this.video.videoHeight > 0) {
        // 关闭原生日志以消除警告
        const originalError = console.error;
        const originalWarn = console.warn;
        
        console.error = function() {};
        console.warn = function() {};
        
        try {
          this.results = this.handLandmarker.detectForVideo(
            this.video,
            startTimeMs,
          );
        } finally {
          console.error = originalError;
          console.warn = originalWarn;
        }
      }
    }

    if (
      this.results &&
      this.results.landmarks &&
      this.results.landmarks.length > 0
    ) {
      const tip = this.results.landmarks[0][8]; // 食指尖
      return { x: tip.x, y: tip.y };
    }
    return null;
  }
}
