import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

// 针对 VRoid 标准表情基的映射
const VROID_MAP = {
  mouthOpen: "Fcl_MTH_A",
  mouthI: "Fcl_MTH_I",
  mouthU: "Fcl_MTH_U",
  mouthE: "Fcl_MTH_E",
  mouthO: "Fcl_MTH_O",
  happy: "Fcl_ALL_Joy",
  sad: "Fcl_ALL_Sorrow",
  angry: "Fcl_ALL_Angry",
  blink: "Fcl_EYE_Close",
  joy: "Fcl_ALL_Joy",
  browJoy: "Fcl_BRW_Joy",
};

export class DigitalHuman {
  constructor(scene) {
    this.scene = scene;
    this.model = null;
    this.morphMeshes = [];
    this.bones = {};
    this.bones.fingersR = [];
    this.isSpeaking = false;
  }

  async load(url) {
    const loader = new GLTFLoader();
    const gltf = await loader.loadAsync(url);
    this.model = gltf.scene;

    this.model.traverse((child) => {
      if (child.isMesh && child.morphTargetDictionary) {
        this.morphMeshes.push(child);
      }
      if (child.isBone) {
        const lowName = child.name.toLowerCase();
        if (lowName.includes("neck")) this.bones.neck = child;
        if (lowName.includes("head")) this.bones.head = child;
        if (lowName.includes("hips") || lowName.includes("pelvis"))
          this.bones.hips = child;
        if (lowName.includes("spine")) this.bones.spine = child;

        // 右臂骨骼捕获
        if (
          lowName.includes("arm") &&
          (lowName.includes("right") || lowName.includes("_r"))
        ) {
          if (lowName.includes("upper")) this.bones.upperArmR = child;
          if (lowName.includes("lower")) this.bones.lowerArmR = child;
        }
        if (
          lowName.includes("hand") &&
          (lowName.includes("right") || lowName.includes("_r"))
        ) {
          this.bones.handR = child;
        }
        // 右手手指 (排除末端)
        if (
          (lowName.includes("right") || lowName.includes("_r")) &&
          (lowName.includes("index") ||
            lowName.includes("middle") ||
            lowName.includes("ring") ||
            lowName.includes("little"))
        ) {
          if (!lowName.includes("end")) this.bones.fingersR.push(child);
        }

        // 左臂 (用于自然摆动)
        if (
          lowName.includes("upperarm") &&
          (lowName.includes("left") || lowName.includes("_l"))
        ) {
          this.bones.upperArmL = child;
        }
      }
    });

    this.model.position.y = 0;
    this.model.rotation.y = 0.3; // 面向左侧
    this.scene.add(this.model);
  }

  setExpression(name, value) {
    const vroidName = VROID_MAP[name] || name;
    this.morphMeshes.forEach((mesh) => {
      const index = mesh.morphTargetDictionary[vroidName];
      if (index !== undefined) mesh.morphTargetInfluences[index] = value;
    });
  }

  update() {
    if (!this.model) return;
    const time = Date.now() * 0.001;

    // 1. 基础呼吸与身体微动
    this.model.position.y = Math.sin(time * 1.0) * 0.01; // 增加位移
    this.model.rotation.z = Math.sin(time * 0.5) * 0.03; // 增加侧向晃动

    if (this.bones.hips) {
      this.bones.hips.rotation.y = Math.sin(time * 0.6) * 0.15; // 骨盆摆动
    }
    if (this.bones.spine) {
      this.bones.spine.rotation.x = Math.sin(time * 0.8) * 0.05; // 脊柱前俯后仰
    }
    if (this.bones.neck) {
      this.bones.neck.rotation.x = 0.1 + Math.sin(time * 0.5) * 0.12; // 进一步低头
      this.bones.neck.rotation.y = Math.sin(time * 0.4) * 0.15;
    }
    if (this.bones.head) {
      this.bones.head.rotation.z = Math.sin(time * 0.4) * 0.2; // 增加歪头幅度
    }

    // 2. 捂嘴笑动作逻辑 (抬手循环)
    const cycle = (time * 1.8) % (Math.PI * 8);
    let liftProgress = 0;

    if (cycle < Math.PI) {
      liftProgress = Math.sin(cycle * 0.5); // 抬起
    } else if (cycle < Math.PI * 2) {
      liftProgress = 1.0; // 保持捂嘴
    } else if (cycle < Math.PI * 3) {
      liftProgress = 1.0 - Math.sin((cycle - Math.PI * 2) * 0.5); // 放下
    }

    // 3. 右手臂姿态驱动 (核心动作: 捂嘴笑)
    if (this.bones.upperArmR && this.bones.lowerArmR) {
      // --- 大臂 (UpperArm) ---
      this.bones.upperArmR.rotation.z =
        1.3 * (1 - liftProgress) + 0.7 * liftProgress;
      this.bones.upperArmR.rotation.x = -0.6 * liftProgress;
      this.bones.upperArmR.rotation.y = 1.5 * liftProgress;

      // --- 前臂 (LowerArm) ---
      this.bones.lowerArmR.rotation.x = -4.1 * liftProgress;
      this.bones.lowerArmR.rotation.y = -2.2 * liftProgress; // 修正手掌指向面部
    }

    if (this.bones.handR) {
      // 手腕微调：让手掌更贴合脸部角度，覆盖嘴部
      this.bones.handR.rotation.z = 0.2 * liftProgress;
      this.bones.handR.rotation.x = 1.8 * liftProgress;
      this.bones.handR.rotation.y = -0.4 * liftProgress;
    }

    // 手指：保持优雅的微握或摊平
    this.bones.fingersR.forEach((f) => {
      f.rotation.z = -0.3 * liftProgress;
    });

    // 4. 左手臂保持自然下垂摆动
    if (this.bones.upperArmL) {
      this.bones.upperArmL.rotation.z = -1.3;
      this.bones.upperArmL.rotation.x = Math.sin(time * 1.2) * 0.05;
    }

    // 5. 表情同步
    // 基础表情：眉毛上扬和微微眯眼
    this.setExpression("Fcl_BRW_Joy", 0.6);
    this.setExpression("Fcl_EYE_Natural", 0.4);

    if (this.isSpeaking) {
      this.setExpression("Fcl_MTH_Joy", 0.7); // 边笑边说
      const vA = (Math.sin(time * 12) + 1) * 0.4;
      this.setExpression("mouthOpen", vA);
    } else {
      // 捂嘴时加大微笑弧度
      const smileVal = 0.5 + 0.4 * liftProgress;
      this.setExpression("Fcl_MTH_Joy", smileVal);
      this.setExpression("mouthOpen", 0);
    }

    // 6. 随机眨眼逻辑
    const blink = Math.sin(time * 4);
    this.setExpression("blink", blink > 0.98 ? 1.0 : 0);
  }

  setSpeaking(state) {
    this.isSpeaking = state;
  }
}
