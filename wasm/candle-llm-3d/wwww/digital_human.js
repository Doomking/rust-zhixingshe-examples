import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
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
};

export class DigitalHuman {
  constructor(scene) {
    this.scene = scene;
    this.model = null;
    this.morphMeshes = [];
    this.bones = {};

    // 说话状态控制
    this.isSpeaking = false;
    this.speakTime = 0;
  }

  async load(url) {
    const loader = new GLTFLoader();
    console.log(`[Avatar] Loading model from: ${url}...`);
    const gltf = await loader.loadAsync(url);
    this.model = gltf.scene;

    console.log("[Avatar] Model Structure Scan Start:");
    this.model.traverse((child) => {
      // 1. 捕获所有表情基 Mesh (可能存在 Face_0, Face_1 等克隆)
      if (child.isMesh && child.morphTargetDictionary) {
        console.log(`[Avatar] Found Morph Mesh: ${child.name}`);
        this.morphMeshes.push(child);

        // 打印表情基关键字，方便调试
        if (this.morphMeshes.length === 1) {
          console.log("[Avatar] Morph Keys Sample:", Object.keys(child.morphTargetDictionary));
        }
      }

      // 2. 捕获关键骨骼
      if (child.isBone) {
        const name = child.name;
        const lowName = name.toLowerCase();

        // 核心躯干
        if (lowName.includes("neck")) this.bones.neck = child;
        if (lowName.includes("head") && !lowName.includes("tail")) this.bones.head = child;
        if (lowName.includes("hips") || lowName.includes("pelvis")) this.bones.hips = child;
        if (lowName.includes("spine")) this.bones.spine = child;

        // 手臂
        if (lowName.includes("arm") && (lowName.includes("left") || lowName.includes("_l")) && !lowName.includes("lower")) {
          this.bones.upperArmL = child;
        }
        if (lowName.includes("arm") && (lowName.includes("right") || lowName.includes("_r")) && !lowName.includes("lower")) {
          this.bones.upperArmR = child;
        }

        // 腿部 (增加灵动感)
        if (lowName.includes("leg") && (lowName.includes("left") || lowName.includes("_l")) && !lowName.includes("upper")) {
          this.bones.legL = child;
        }
        if (lowName.includes("leg") && (lowName.includes("right") || lowName.includes("_r")) && !lowName.includes("upper")) {
          this.bones.legR = child;
        }

        // 下臂 (用于更复杂的“手抬起来”动作)
        if (lowName.includes("lower") && lowName.includes("arm")) {
          if (lowName.includes("left") || lowName.includes("_l")) this.bones.lowerArmL = child;
          if (lowName.includes("right") || lowName.includes("_r")) this.bones.lowerArmR = child;
        }
      }
    });

    console.log("[Avatar] Found Bones:", Object.keys(this.bones));
    if (this.morphMeshes.length === 0) console.warn("[Avatar] No Morph Mesh found!");

    // 测量与对齐
    const box = new THREE.Box3().setFromObject(this.model);
    const size = new THREE.Vector3();
    box.getSize(size);
    console.log(`[Avatar] Model Size: Width=${size.x.toFixed(2)}, Height=${size.y.toFixed(2)}`);

    // 3. 初始姿态 (此处仅做初始化，动态都在 update 里驱动)
    this.model.position.y = 0;
    // 身体向右侧稍微偏转 (Viewer's Right) -> 也就是绕Y轴负向旋转
    this.model.rotation.y = -0.3;
    this.scene.add(this.model);
    console.log("[Avatar] Load Complete.");
  }

  // 设置表情 (驱动所有相关 Mesh)
  setExpression(name, value) {
    const vroidName = VROID_MAP[name] || name;
    this.morphMeshes.forEach(mesh => {
      const index = mesh.morphTargetDictionary[vroidName];
      if (index !== undefined) {
        mesh.morphTargetInfluences[index] = value;
      }
    });
  }

  setSpeaking(speaking) {
    this.isSpeaking = speaking;
  }

  update() {
    if (!this.model) return;

    const time = Date.now() * 0.001;
    this.speakTime += 0.16; // 说话动画进动速度

    // 1. 拒绝“飘动”：极大幅度削减 Y 轴位移，改为重心偏移 (Grounding)
    // 之前是 0.012，现在改为 0.001 (更稳)
    const breath = Math.sin(time * 0.8) * 0.001;
    this.model.position.y = breath;
    // 保持基础旋转 (0.3) 并叠加微弱摆动
    this.model.rotation.y = 0.3 + Math.sin(time * 0.3) * 0.02;

    // 骨盆与脊柱的随动 (减小幅度，避免过度扭动)
    if (this.bones.hips) {
      this.bones.hips.rotation.y = Math.sin(time * 0.4) * 0.05; // 0.08 -> 0.05
      this.bones.hips.position.x = Math.cos(time * 0.4) * 0.005;
    }
    if (this.bones.spine) {
      this.bones.spine.rotation.z = Math.sin(time * 0.3) * 0.05;
    }

    // 2. 头部与颈部 (低头示意，增加亲近感)
    if (this.bones.neck) {
      this.bones.neck.rotation.y = Math.sin(time * 0.5) * 0.1;
      this.bones.neck.rotation.x = Math.cos(time * 0.3) * 0.05 + 0.15; // 0.15: 显著低头
    }
    if (this.bones.head) {
      // 头部大幅度左右晃动 (卖萌)
      this.bones.head.rotation.y = Math.sin(time * 0.4) * 0.2; // 幅度增大
      this.bones.head.rotation.z = Math.sin(time * 0.3) * 0.1;
    }

    // 3. 腿部与手臂微动
    if (this.bones.legL) this.bones.legL.rotation.y = Math.sin(time * 0.3) * 0.05;
    if (this.bones.legR) this.bones.legR.rotation.y = Math.cos(time * 0.3) * 0.05;

    // 手臂动作：循环抬手到脸部，再放下 (Hand to Face Cycle)
    // 周期：抬起(2s) -> 保持(1s) -> 放下(1s) -> 休息(2s)
    let armCycle = (time * 0.5) % (Math.PI * 4); // 减慢周期

    let armLift = 0; // 0: 下垂, 1: 抬起

    // 简单状态机模拟
    if (armCycle < Math.PI) {
      // 抬起阶段
      armLift = Math.sin(armCycle * 0.5);
    } else if (armCycle < Math.PI * 1.5) {
      // 保持阶段
      armLift = 1.0;
    } else if (armCycle < Math.PI * 2.5) {
      // 放下阶段
      armLift = 1.0 - Math.sin((armCycle - Math.PI * 1.5) * 0.5);
    } else {
      // 休息阶段
      armLift = 0;
    }

    if (this.bones.upperArmR) {
      // Upper Arm: X轴负向是前 (通常), Z轴正向是向外张开
      // 基础位置 (下垂): x=0, z=1.25(贴身)
      // 目标位置 (抬手): x=-0.8(向前抬), z=0.6(抬高)
      const baseX = Math.sin(time * 0.8) * 0.05;
      const targetX = -0.5; // 向前抬

      const baseZ = 1.25;
      const targetZ = 0.5; // 抬起

      this.bones.upperArmR.rotation.x = baseX * (1 - armLift) + targetX * armLift;
      this.bones.upperArmR.rotation.z = baseZ * (1 - armLift) + targetZ * armLift;
    }

    if (this.bones.lowerArmR) {
      // Lower Arm: 这是一个单纯的弯曲关节
      // 休息: x=0 (伸直)
      // 目标: x=-2.2 (折叠)
      const targetBend = -2.3;
      this.bones.lowerArmR.rotation.x = targetBend * armLift;

      // 手掌微调
      this.bones.lowerArmR.rotation.y = -0.5 * armLift;
      this.bones.lowerArmR.rotation.z = Math.sin(time * 2) * 0.05; // 手部微颤
      // 如果轴向不对（不同模型可能不同），可能需要调整 Y 轴
      // this.bones.lowerArmR.rotation.y = -1.0; 
    }

    if (this.bones.upperArmL) {
      // 左手自然下垂
      this.bones.upperArmL.rotation.z = -1.25;
      this.bones.upperArmL.rotation.x = Math.sin(time * 1.5) * 0.03;
    }

    // 4. 表情与口型驱动 (Advanced Lip Sync)
    // 基础表情：保持眉毛和眼睛的笑意
    this.setExpression("Fcl_ALL_Joy", 0);
    this.setExpression("Fcl_BRW_Joy", 0.5);
    this.setExpression("Fcl_EYE_Natural", 0.5);

    if (this.isSpeaking) {
      // 说话时：保留较多微笑，让表情更亲和 (之前是 0.2，太严肃了)
      this.setExpression("Fcl_MTH_Joy", 0.6);

      // 模拟元音随机切换 (A, I, U, E, O)
      // 使用正弦波组合来模拟语流
      const t = time * 15.0;
      const vA = (Math.sin(t) + 1) * 0.5;
      const vI = (Math.sin(t + 2) + 1) * 0.5;
      const vU = (Math.sin(t + 4) + 1) * 0.5;

      // 归一化权重并应用
      this.setExpression("mouthOpen", vA * 0.6); // A
      this.setExpression("mouthI", vI * 0.4);    // I
      this.setExpression("mouthO", vU * 0.5);    // U/O
    } else {
      // 不说话时：恢复甜美微笑
      this.setExpression("Fcl_MTH_Joy", 0.8);
      this.setExpression("mouthOpen", 0);
      this.setExpression("mouthI", 0);
      this.setExpression("mouthO", 0);
    }

    // 眨眼逻辑优化：更像真人的瞬时眨眼 (Sharp Blink)
    // 使用高频正弦波叠加阈值截断，产生快速闭合效果
    const blinkTrigger = Math.sin(time * 3.5); // 频率提高
    // 只有在波峰极小段才眨眼
    if (blinkTrigger > 0.992) {
      this.setExpression("Fcl_EYE_Close", 1.0); // 瞬间全闭
    } else if (blinkTrigger > 0.98) {
      this.setExpression("Fcl_EYE_Close", 0.5); // 过渡
    } else {
      this.setExpression("Fcl_EYE_Close", 0.0); // 平时睁开
    }

    // 手臂动作：循环抬手到脸部，再放下 (Hand to Face Cycle)
    // 周期：抬起(1.5s) -> 保持(1.5s) -> 放下(1s) -> 休息(2s)
    armCycle = (time * 0.6) % (Math.PI * 4);

    armLift = 0; // 0: 下垂, 1: 抬起

    if (armCycle < Math.PI) {
      // 抬起阶段 (Sin 0->1)
      armLift = Math.sin(armCycle * 0.5);
    } else if (armCycle < Math.PI * 2.0) {
      // 保持阶段
      armLift = 1.0;
    } else if (armCycle < Math.PI * 3.0) {
      // 放下阶段 (Sin 1->0)
      armLift = 1.0 - Math.sin((armCycle - Math.PI * 2.0) * 0.5);
    } else {
      // 休息阶段
      armLift = 0;
    }

    if (this.bones.upperArmR) {
      // Upper Arm: 
      // 基础(Rest): X=Math.sin(time)*0.03 (轻微摆动), Z=1.25 (下垂)
      // 抬起(Lift): X=-0.8 (大幅前抬), Z=0.6 (抬高), Y=-0.8 (强力内收)

      const restX = Math.sin(time * 0.8) * 0.05;
      const liftX = -0.8; // 更向里/前抬

      const restZ = 1.25;
      const liftZ = 0.6;

      const liftY = -0.8; // 转腕向脸

      this.bones.upperArmR.rotation.x = restX * (1 - armLift) + liftX * armLift;
      this.bones.upperArmR.rotation.z = restZ * (1 - armLift) + liftZ * armLift;
      this.bones.upperArmR.rotation.y = liftY * armLift;
    }

    if (this.bones.lowerArmR) {
      // Lower Arm: 
      // Rest: X=0
      // Lift: X=-2.3 (向上折叠)
      const liftBend = -2.3;
      this.bones.lowerArmR.rotation.x = liftBend * armLift;
      this.bones.lowerArmR.rotation.y = -0.5 * armLift;
      this.bones.lowerArmR.rotation.z = Math.sin(time * 5) * 0.05 * armLift; // 只有抬起时手才颤动
    }

    // 左手保持自然摆动
    if (this.bones.upperArmL) {
      this.bones.upperArmL.rotation.z = -1.25;
      this.bones.upperArmL.rotation.x = Math.sin(time * 1.5) * 0.03;
    }
  }

  resetExpressions() {
    this.morphMeshes.forEach(mesh => {
      if (mesh.morphTargetInfluences) mesh.morphTargetInfluences.fill(0);
    });
  }
}
