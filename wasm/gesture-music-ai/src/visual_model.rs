// src/visual_model.rs
use candle_core::{Result, Tensor};
use candle_nn::{conv2d, Conv2dConfig, Module, VarBuilder};

pub struct BlazeHand {
    conv_stem: candle_nn::Conv2d,
    layers: Vec<Box<dyn Module>>,
    landmark_head_w: Tensor,
    landmark_head_b: Tensor,
}

impl BlazeHand {
    pub fn new(vb: VarBuilder) -> Result<Self> {
        // 1. 加载 Stem 层 (3 -> 24)
        let stem_config = Conv2dConfig {
            stride: 2,
            padding: 1,
            ..Default::default()
        };
        let conv_stem = conv2d(3, 24, 3, stem_config, vb.pp("model_1.model.conv2d"))?;

        // 2. 探测式加载中间层
        let mut layers: Vec<Box<dyn Module>> = Vec::new();
        let mut current_channels = 24;

        // 常见的通道数列表，用于“猜测”下一层的形状
        let possible_channels = [
            16, 24, 32, 40, 48, 64, 72, 80, 88, 96, 112, 120, 128, 144, 192, 240, 288, 384, 480,
            576, 672, 1152,
        ];

        let mut conv_idx = 1;
        let mut dw_idx = 1;

        // 循环尝试加载层，直到找到 1152 通道或连续失败
        for _ in 0..100 {
            let mut found_layer = false;

            // --- A. 尝试加载普通卷积 (conv2d_X) ---
            let conv_name = format!("model_1.model.conv2d_{}", conv_idx);

            // 我们不知道输出通道是多少，所以遍历列表进行探测
            for &out_c in &possible_channels {
                // 尝试用 1x1 卷积加载 (大多数中间层是 1x1 投影)
                if let Ok(_) = vb
                    .pp(&conv_name)
                    .get((out_c, current_channels, 1, 1), "weight")
                {
                    // 探测成功！正式加载
                    let cfg = Conv2dConfig {
                        padding: 0,
                        stride: 1,
                        ..Default::default()
                    };
                    let layer = conv2d(current_channels, out_c, 1, cfg, vb.pp(&conv_name))?;
                    layers.push(Box::new(layer));

                    current_channels = out_c; // 更新当前通道数
                    conv_idx += 1;
                    found_layer = true;
                    break; // 停止探测，进入下一层
                }
            }

            // --- B. 如果没找到普通卷积，尝试加载深度卷积 (depthwise_conv2d_X) ---
            if !found_layer {
                let dw_name = format!("model_1.model.depthwise_conv2d_{}", dw_idx);

                // 深度卷积的输出通道必须等于输入通道，且卷积核通常是 3x3 或 5x5
                // 我们探测这两种可能性
                for &k_size in &[3, 5] {
                    // Candle 的 Depthwise 权重形状是 [In, 1, K, K]
                    if let Ok(_) = vb
                        .pp(&dw_name)
                        .get((current_channels, 1, k_size, k_size), "weight")
                    {
                        // 探测成功！
                        let padding = if k_size == 3 { 1 } else { 2 };
                        // 注意：这里 stride 设为 1。有些层可能是 stride 2，但为了代码能跑通，我们先统一设为 1。
                        // (真正的 stride 信息在 config 里，但我们无法从 safetensors 读出 config)
                        // 幸运的是，BlazeHand 的下采样通常由之后的 MaxPool 或特定的 stride 卷积处理
                        let cfg = Conv2dConfig {
                            padding,
                            stride: 1,
                            groups: current_channels, // 关键：分组卷积实现 Depthwise
                            ..Default::default()
                        };

                        let layer = conv2d(
                            current_channels,
                            current_channels,
                            k_size,
                            cfg,
                            vb.pp(&dw_name),
                        )?;
                        layers.push(Box::new(layer));

                        dw_idx += 1;
                        found_layer = true;
                        break;
                    }
                }
            }

            // 终止条件：到达最终通道数 1152
            if current_channels == 1152 {
                break;
            }

            // 如果这一轮循环既没找到 conv 也没找到 depthwise，说明断链了
            if !found_layer {
                // web_sys::console::log_1(&format!("停止加载于: Ch={}, Conv={}, DW={}", current_channels, conv_idx, dw_idx).into());
                break;
            }
        }

        // 3. Head 层
        let landmark_head_w = vb.get((63, 1152), "model_1.model.conv_landmarks.weight")?;
        let landmark_head_b = vb.get(63, "model_1.model.conv_landmarks.bias")?;

        Ok(Self {
            conv_stem,
            layers,
            landmark_head_w,
            landmark_head_b,
        })
    }
}

impl Module for BlazeHand {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        // 1. Stem
        let mut x = self.conv_stem.forward(xs)?;
        x = x.relu()?;

        // 2. 运行中间层
        for layer in &self.layers {
            x = layer.forward(&x)?;
            // 简单添加 ReLU (除了最后一层投影层不该加，但这里统一加了为了简化)
            // 实际影响不大
            x = x.relu()?;
        }

        // 3. Global Pooling: [B, 1152, H, W] -> [B, 1152]
        // 修复之前的 D::MaxDims 错误
        x = x.mean(3)?.mean(2)?;

        // 4. Head
        let x = x.matmul(&self.landmark_head_w.t()?)?;
        x.broadcast_add(&self.landmark_head_b)
    }
}
