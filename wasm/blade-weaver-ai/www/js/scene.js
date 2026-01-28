import * as THREE from "three";

// ============ 水果配置系统 ============
const FRUIT_CONFIGS = {
  apple: {
    geometry: (size) => {
      const geo = new THREE.SphereGeometry(size, 32, 32);
      return geo;
    },
    color: 0xe63946,
    specular: 0xffd700,
    shininess: 10,
    size: 0.5,
    mass: 1.0,
    rotationSpeed: 0.02,
    details: true
  },
  pear: {
    geometry: (size) => {
      const geo = new THREE.SphereGeometry(size, 32, 32);
      return geo;
    },
    color: 0x8fb996,
    specular: 0xd4a373,
    shininess: 15,
    size: 0.55,
    mass: 1.1,
    rotationSpeed: 0.015,
    details: true
  },
  banana: {
    geometry: (size) => {
      const geo = new THREE.CylinderGeometry(size * 0.3, size * 0.2, size * 1.5, 16);
      geo.rotateX(Math.PI * 0.5);
      return geo;
    },
    color: 0xfefae0,
    specular: 0xffd700,
    shininess: 20,
    size: 0.4,
    mass: 0.8,
    rotationSpeed: 0.025,
    details: true
  },
  watermelon: {
    geometry: (size) => {
      const geo = new THREE.SphereGeometry(size, 32, 32);
      return geo;
    },
    color: 0x2a9d8f,
    specular: 0x264653,
    shininess: 5,
    size: 0.7,
    mass: 1.5,
    rotationSpeed: 0.01,
    details: true
  },
  strawberry: {
    geometry: (size) => {
      const geo = new THREE.SphereGeometry(size, 32, 32);
      return geo;
    },
    color: 0xe76f51,
    specular: 0xfff8e1,
    shininess: 10,
    size: 0.35,
    mass: 0.6,
    rotationSpeed: 0.03,
    details: true
  }
};

// ============ 物理配置 ============
const PHYSICS_CONFIG = {
  maxTrailPoints: 20,
  updateInterval: 16,
  collisionThreshold: 0.6,
  fruitGravity: -0.0012,
  fruitAirResistance: 0.995,
  fruitFriction: 0.99,
  deltaTimeTarget: 0.016, // 60fps
  sliceForce: 0.05
};

// ============ 对象池类 ============
class ObjectPool {
  constructor(size) {
    this.size = size;
    this.available = [];
    this.inUse = new Set();
    
    // 预分配对象
    for (let i = 0; i < size; i++) {
      this.available.push({
        mesh: null,
        position: new THREE.Vector3(),
        velocity: new THREE.Vector3(),
        userData: {},
      });
    }
  }

  get() {
    if (this.available.length > 0) {
      const obj = this.available.pop();
      this.inUse.add(obj);
      return obj;
    }
    return null;
  }

  release(obj) {
    if (this.inUse.has(obj)) {
      this.inUse.delete(obj);
      this.available.push(obj);
    }
  }

  releaseAll() {
    this.inUse.forEach(obj => this.release(obj));
  }

  getAvailableCount() {
    return this.available.length;
  }
}

// ============ 水果工厂类 ============
class FruitFactory {
  static getRandomFruitType() {
    const types = Object.keys(FRUIT_CONFIGS);
    return types[Math.floor(Math.random() * types.length)];
  }

  static createFruit(type) {
    const config = FRUIT_CONFIGS[type];
    if (!config) {
      throw new Error(`Unknown fruit type: ${type}`);
    }

    const size = config.size * (0.9 + Math.random() * 0.2); // 随机大小变化
    const geometry = config.geometry(size);
    
    // 创建更逼真的材质
    const material = new THREE.MeshPhongMaterial({
      color: config.color,
      specular: config.specular || 0xffffff,
      shininess: config.shininess || 10,
      side: THREE.DoubleSide
    });
    
    const fruit = new THREE.Mesh(geometry, material);

    fruit.userData = {
      type: type,
      mass: config.mass,
      rotationSpeed: config.rotationSpeed,
      sliced: false,
      color: config.color,
      specular: config.specular || 0xffffff
    };

    return fruit;
  }
}

// ============ 距离预检函数 ============
function distancePreCheck(pos1, pos2, quickRejectDist) {
  const dx = pos1.x - pos2.x;
  const dy = pos1.y - pos2.y;
  const distSq = dx * dx + dy * dy;
  return distSq < quickRejectDist * quickRejectDist;
}

export class GameScene {
  constructor(canvas, rustCore) {
    this.canvas = canvas;
    this.rustCore = rustCore; // 传入初始化好的 WASM BladeCore 实例

    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(
      75,
      window.innerWidth / window.innerHeight,
      0.1,
      1000,
    );
    this.renderer = new THREE.WebGLRenderer({
      canvas: this.canvas,
      antialias: true,
      alpha: true,
    });
    this.renderer.setClearColor(0x000000, 0); // 设置清除颜色为透明

    this.fruits = [];
    this.bladeTrail = null;
    this.trailPoints = [];
    this.maxTrailPoints = PHYSICS_CONFIG.maxTrailPoints;
    this.lastUpdateTime = 0;
    this.updateInterval = PHYSICS_CONFIG.updateInterval;
    
    // Delta Time 相关变量
    this.lastFrameTime = Date.now();
    this.deltaTime = 0;
    
    // 对象池：预分配200个水果对象
    this.objectPool = new ObjectPool(200);
    
    // 平滑的手势位置
    this.smoothedHandPos = new THREE.Vector3(0, 0, 0);

    this.init();
  }

  init() {
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    this.camera.position.z = 5;

    // 光照
    const ambientLight = new THREE.AmbientLight(0xffffff, 0.6);
    const directionalLight = new THREE.DirectionalLight(0xffffff, 1);
    directionalLight.position.set(0, 5, 5);
    this.scene.add(ambientLight, directionalLight);

    // 创建刀锋轨迹的几何体（主线条，更粗更亮）
    const trailGeometry = new THREE.BufferGeometry();
    const trailMaterial = new THREE.LineBasicMaterial({
      color: 0x00ffff,
      linewidth: 10,
      transparent: true,
      opacity: 0.9,
    });
    this.bladeTrail = new THREE.Line(trailGeometry, trailMaterial);
    this.scene.add(this.bladeTrail);
    
    // 创建刀光辉光效果（辅助线，增强视觉）
    this.createBladeGlow();

    window.addEventListener("resize", () => this.onWindowResize());
  }

  // 创建刀光辉光效果
  createBladeGlow() {
    const glowGeometry = new THREE.BufferGeometry();
    const glowMaterial = new THREE.LineBasicMaterial({
      color: 0x00ff88,
      linewidth: 2,
      transparent: true,
      opacity: 0.3,
    });
    this.bladeGlow = new THREE.Line(glowGeometry, glowMaterial);
    this.scene.add(this.bladeGlow);
  }

  // 核心更新函数：由 main.js 在 requestAnimationFrame 中调用
  update(rawHandPos) {
    // 计算 deltaTime
    const now = Date.now();
    this.deltaTime = (now - this.lastFrameTime) / 1000.0; // 转换为秒
    this.lastFrameTime = now;
    
    if (!rawHandPos) {
      this.clearTrail();
      return;
    }

    // 1. 调用 Rust 进行卡尔曼滤波平滑处理
    const smoothed = this.rustCore.update_hand(rawHandPos.x, rawHandPos.y);

    // 2. 坐标转换：MediaPipe (0~1) -> Three.js 屏幕空间 (-N ~ N)
    const v3Pos = new THREE.Vector3(
      (1 - smoothed[0]) * 10 - 5, // 镜像并缩放
      -(smoothed[1] * 8 - 4),
      0,
    );
    
    // 保存平滑的手势位置
    this.smoothedHandPos.copy(v3Pos);

    this.updateTrail(v3Pos);
    this.checkCollisions(v3Pos);
    this.animateFruits();
    this.renderer.render(this.scene, this.camera);
  }

  updateTrail(pos) {
    try {
      // 验证位置数据是否有效
      if (!pos || isNaN(pos.x) || isNaN(pos.y) || isNaN(pos.z)) {
        this.clearTrail();
        return;
      }

      // 限制轨迹点数量，减少计算
      this.trailPoints.push(pos);
      if (this.trailPoints.length > this.maxTrailPoints) {
        this.trailPoints.shift();
      }

      // 只处理最近的点，减少计算量
      const recentPoints = this.trailPoints.slice(-10); // 只取最近的10个点
      
      if (recentPoints.length > 1) {
        // 复用现有的 Float32Array，减少内存分配
        const vertices = new Float32Array(recentPoints.length * 3);
        for (let i = 0; i < recentPoints.length; i++) {
          const p = recentPoints[i];
          vertices[i * 3] = p.x;
          vertices[i * 3 + 1] = p.y;
          vertices[i * 3 + 2] = p.z;
        }
        
        // 直接更新现有属性，避免创建新的 BufferAttribute
        if (this.bladeTrail.geometry.attributes.position) {
          this.bladeTrail.geometry.attributes.position.array = vertices;
          this.bladeTrail.geometry.attributes.position.count = vertices.length / 3;
        } else {
          this.bladeTrail.geometry.setAttribute(
            "position",
            new THREE.BufferAttribute(vertices, 3)
          );
        }
        this.bladeTrail.geometry.attributes.position.needsUpdate = true;
        
        // 简化辉光，减少性能开销
        if (this.bladeGlow && recentPoints.length > 2) {
          // 只取最近的5个点构建辉光
          const glowPoints = recentPoints.slice(-5);
          const glowVertices = new Float32Array(glowPoints.length * 3);
          for (let i = 0; i < glowPoints.length; i++) {
            const p = glowPoints[i];
            glowVertices[i * 3] = p.x;
            glowVertices[i * 3 + 1] = p.y;
            glowVertices[i * 3 + 2] = p.z + 0.1;
          }
          
          if (this.bladeGlow.geometry.attributes.position) {
            this.bladeGlow.geometry.attributes.position.array = glowVertices;
            this.bladeGlow.geometry.attributes.position.count = glowVertices.length / 3;
          } else {
            this.bladeGlow.geometry.setAttribute(
              "position",
              new THREE.BufferAttribute(glowVertices, 3)
            );
          }
          this.bladeGlow.geometry.attributes.position.needsUpdate = true;
        }
      } else {
        this.clearTrail();
      }
    } catch (error) {
      console.error("❌ 更新轨迹失败:", error);
      this.clearTrail();
    }
  }

  checkCollisions(bladePos) {
    // 只处理有限数量的水果，避免过多计算
    const maxChecks = 20;
    let checked = 0;
    
    for (let i = this.fruits.length - 1; i >= 0 && checked < maxChecks; i--) {
      const fruit = this.fruits[i];
      if (fruit.userData.sliced) continue;

      // 距离预检：先做快速的平方距离检查，避免计算昂贵的平方根
      if (!distancePreCheck(bladePos, fruit.position, PHYSICS_CONFIG.collisionThreshold)) {
        continue;
      }
      
      checked++;
      const dist = bladePos.distanceTo(fruit.position);
      if (dist < PHYSICS_CONFIG.collisionThreshold) {
        this.performSlice(fruit);
        // 每次只切割一个水果，减少同一帧的计算量
        break;
      }
    }
  }

  // 调用 Rust Geometry 模块进行模型切分
  performSlice(fruit) {
    try {
      fruit.userData.sliced = true;

      // 获取当前位置的数据
      const vertices = fruit.geometry.attributes.position.array;

      // 计算切割平面：法线基于手势移动方向 (由 Rust 计算)
      const normal = this.rustCore.calculate_slice_plane([
        fruit.position.x,
        fruit.position.y,
      ]);
      const point = [0, 0, 0]; // 局部空间原点

      // 调用 Rust 的计算几何算法
      const result = this.rustCore.slice_mesh(vertices, normal, point);

      // 隐藏原水果
      this.scene.remove(fruit);
      this.fruits.splice(this.fruits.indexOf(fruit), 1);
      
      // 释放对象池中的对象
      if (fruit.userData.poolObject) {
        this.objectPool.release(fruit.userData.poolObject);
      }

      // 保存水果材质属性
      const fruitColor = fruit.material.color.getHex();
      const fruitSpecular = fruit.material.specular ? fruit.material.specular.getHex() : 0xffffff;
      const fruitShininess = fruit.material.shininess || 10;
      
      // 创建两个半块
      this.createHalf(result.mesh_a, fruit.position, normal, 1, fruitColor, fruitSpecular, fruitShininess);
      this.createHalf(result.mesh_b, fruit.position, normal, -1, fruitColor, fruitSpecular, fruitShininess);

      // 触发游戏事件（给 LLM 导演发消息）
      window.dispatchEvent(
        new CustomEvent("fruit-sliced", { detail: { type: fruit.userData.type } }),
      );
    } catch (error) {
      console.error("❌ 切割水果失败:", error);
      
      // 即使切割失败，也要移除原水果，避免游戏卡住
      try {
        this.scene.remove(fruit);
        this.fruits.splice(this.fruits.indexOf(fruit), 1);
        
        // 释放对象池中的对象
        if (fruit.userData.poolObject) {
          this.objectPool.release(fruit.userData.poolObject);
        }
      } catch (removeError) {
        console.error("❌ 移除水果失败:", removeError);
      }
    }
  }

  createHalf(vertexData, position, normal, direction, fruitColor, fruitSpecular, fruitShininess) {
    if (vertexData.length === 0) return;

    // 过滤掉包含 NaN 的顶点数据
    const filteredData = [];
    let hasValidData = false;
    
    for (let i = 0; i < vertexData.length; i += 3) {
      const x = vertexData[i];
      const y = vertexData[i + 1];
      const z = vertexData[i + 2];
      
      // 检查是否包含 NaN
      if (!isNaN(x) && !isNaN(y) && !isNaN(z)) {
        filteredData.push(x, y, z);
        hasValidData = true;
      }
    }
    
    // 如果没有有效数据，直接返回
    if (!hasValidData || filteredData.length === 0) {
      console.warn("⚠️ 过滤后没有有效顶点数据，跳过创建半块");
      return;
    }

    try {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute(
        "position",
        new THREE.BufferAttribute(new Float32Array(filteredData), 3),
      );
      
      // 计算边界球体，避免 NaN 错误
      try {
        geometry.computeBoundingSphere();
      } catch (e) {
        console.warn("⚠️ 计算边界球体失败:", e);
      }
      
      // 创建与原始水果一致的材质
      const material = new THREE.MeshPhongMaterial({
        color: fruitColor || 0xff4444,
        specular: fruitSpecular || 0xffffff,
        shininess: fruitShininess || 10,
        side: THREE.DoubleSide,
      });
      const half = new THREE.Mesh(geometry, material);

      half.position.copy(position);
      // 给半块一个向外的推力
      const sliceForce = PHYSICS_CONFIG.sliceForce;
      half.userData = {
        velocity: new THREE.Vector3(
          normal[0] * direction * sliceForce,
          normal[1] * direction * sliceForce,
          -0.05,
        ),
        gravity: PHYSICS_CONFIG.fruitGravity,
        life: 100,
        angularVelocity: new THREE.Vector3(
          (Math.random() - 0.5) * 0.05,
          (Math.random() - 0.5) * 0.05,
          (Math.random() - 0.5) * 0.05
        )
      };

      this.scene.add(half);
      this.fruits.push(half); // 加入队列以便执行掉落动画
    } catch (error) {
      console.error("❌ 创建半块失败:", error);
    }
  }

  spawnFruit() {
    // 尝试从对象池获取对象
    const poolObject = this.objectPool.get();
    
    // 生成随机类型的水果
    const fruitType = FruitFactory.getRandomFruitType();
    const fruit = FruitFactory.createFruit(fruitType);

    // 从屏幕上方出现，随机水平位置
    fruit.position.set((Math.random() - 0.5) * 6, 4, 0);
    
    // 随机初始速度和角度
    const horizontalVelocity = (Math.random() - 0.5) * 0.04;
    const verticalVelocity = -0.03 - Math.random() * 0.03;
    const angularVelocity = new THREE.Vector3(
      (Math.random() - 0.5) * fruit.userData.rotationSpeed * 2,
      (Math.random() - 0.5) * fruit.userData.rotationSpeed * 2,
      (Math.random() - 0.5) * fruit.userData.rotationSpeed * 2
    );

    fruit.userData = {
      ...fruit.userData,
      velocity: new THREE.Vector3(horizontalVelocity, verticalVelocity, 0),
      angularVelocity: angularVelocity,
      gravity: PHYSICS_CONFIG.fruitGravity * fruit.userData.mass,
      sliced: false,
      poolObject: poolObject, // 保存对象池引用以便后续释放
    };

    this.scene.add(fruit);
    this.fruits.push(fruit);
  }

  animateFruits() {
    for (let i = this.fruits.length - 1; i >= 0; i--) {
      const f = this.fruits[i];
      // 使用 deltaTime 进行物理计算
      f.position.add(f.userData.velocity.clone().multiplyScalar(this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget));
      f.userData.velocity.y += f.userData.gravity * (this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget);
      
      // 应用空气阻力
      f.userData.velocity.multiplyScalar(PHYSICS_CONFIG.fruitAirResistance);
      
      // 应用旋转
      if (f.userData.angularVelocity) {
        f.rotation.x += f.userData.angularVelocity.x * (this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget);
        f.rotation.y += f.userData.angularVelocity.y * (this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget);
        f.rotation.z += f.userData.angularVelocity.z * (this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget);
        
        // 旋转阻力
        f.userData.angularVelocity.multiplyScalar(PHYSICS_CONFIG.fruitAirResistance);
      } else {
        //  fallback 旋转
        f.rotation.x += 0.02 * (this.deltaTime / PHYSICS_CONFIG.deltaTimeTarget);
      }

      // 水果掉出屏幕下方时移除
      if (f.position.y < -5) {
        this.scene.remove(f);
        // 释放对象池中的对象
        if (f.userData.poolObject) {
          this.objectPool.release(f.userData.poolObject);
        }
        this.fruits.splice(i, 1);
      }
    }
  }

  clearTrail() {
    this.trailPoints = [];
    this.bladeTrail.geometry.setAttribute(
      "position",
      new THREE.BufferAttribute(new Float32Array(), 3),
    );
  }

  onWindowResize() {
    this.camera.aspect = window.innerWidth / window.innerHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(window.innerWidth, window.innerHeight);
  }

  // 获取当前活跃的水果数量
  getActiveFruitCount() {
    return this.fruits.length;
  }
}
