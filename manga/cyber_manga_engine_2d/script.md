太棒了，吉卜力风格（Studio Ghibli Style）是测试 AI 生成一致性和艺术感的绝佳选择。

这种风格对你的 CyberManga-Engine 提出了完全不同的挑战：它要求**柔和的水彩质感**、**充满细节的自然背景**、以及**温暖治愈的氛围**，而不是赛博朋克的高对比度和霓虹灯。

这份剧本旨在测试你的引擎是否能捕捉到那种“静谧的魔法感”。

---

### 剧本标题：《云端修理屋的午后》 (Afternoon at the Cloud Workshop)

**核心风格指南 (Style Lora 提示词建议)：**

> Studio Ghibli, watercolor texture, hand-painted background, soft natural lighting, nostalgic, warm colors, detailed environment, hayao miyazaki style.

**角色设定：**

* **主角 (Mio / 米奥)：** 16 岁少女，绑着凌乱的麻花辫，穿着沾有少许油污的工装背带裤，戴着大大的防风镜。性格活泼，动手能力强。
* **环境：** 这是一个位于悬崖边、半木制半机械的小工坊，周围环绕着茂密的绿色植物和巨大的风车。

---

### 第一页（共 4 格）

#### **Panel 1：宁静的开场**

* **画面描述 (Prompt 提示)：**
* A wide shot of a cozy, cluttered workshop filled with wooden gears, potted plants, and blueprints. Sunlight streams through a large window.
* Mio is sitting on a stool, focused on polishing a small, brass engine part.
* **Style:** Ghibli watercolor style, incredibly detailed background, peaceful atmosphere.


* **对白 (Bubble)：**
* Mio (独白)：“爷爷说过，这里的风是有脾气的，顺着它才能飞起来。”



#### **Panel 2：小小的挫折**

* **画面描述 (Prompt 提示)：**
* Medium shot. Mio tries to fit the brass part into a larger wooden wing mechanism. It gets stuck.
* A comical puff of white smoke and a small spring pops out.
* Mio has a frustrated, cartoonishly annoyed expression (a classic Ghibli pout).


* **音效字 (SFX)：** 噗 (Poof), 嘣 (Boing)
* **对白 (Bubble)：**
* Mio：“哎呀！又卡住了！”



#### **Panel 3：风的指引**

* **画面描述 (Prompt 提示)：**
* Close up on Mio's face. Her hair and the blueprints on the table are gently lifted by a sudden breeze coming from the window.
* Her expression changes from annoyance to surprise, looking towards the window.
* **Style:** Soft light, emphasis on movement of hair and paper.



#### **Panel 4：仰望奇迹**

* **画面描述 (Prompt 提示)：**
* Mio stands on the balcony looking out.
* A massive, fantastical airship (shaped like a giant fish made of wood and canvas propellers) sails silently and slowly through a sea of fluffy clouds in the distance.
* The scale is huge, showing the wonder of the world.


* **对白 (Bubble)：**
* Mio：“哇……是巡游者号。”


* **心理活动 (Square Bubble)：**
* 总有一天，我也要造出能飞那么远的家伙。



---

### 💡 给 AI 引擎的测试重点

这个剧本与上一个赛博朋克剧本截然不同，它的测试点在于：

1. **纹理一致性：** 你的 Rust 后端需要确保生成的每一张图都像是**用同一种水彩笔画出来的**。如果突然出现一张油画质感的图，就会非常突兀。
2. **细节密度：** Ghibli 风格的背景非常复杂（Panel 1 的杂物间）。测试 Stable Diffusion 能否在保持主角不变形的情况下，稳定生成复杂的背景细节。
3. **氛围渲染：** Panel 4 需要那种**宏大、静谧、治愈**的感觉，考验 Prompt 对光影和构图的把控。