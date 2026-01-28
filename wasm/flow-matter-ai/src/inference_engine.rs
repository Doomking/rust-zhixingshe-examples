use crate::image_processor::ImageProcessor;
use crate::model::MobileSAM;
use candle::{Device, Result, Tensor};
use candle_nn::VarBuilder;

pub struct InferenceEngine {
    model: MobileSAM,
    processor: ImageProcessor,
    device: Device,
    // 关键：缓存当前图片的特征，避免重复计算
    current_image_embedding: Option<Tensor>,
    pub img_dims: (u32, u32),
}

impl InferenceEngine {
    pub fn new(model_data: &[u8], device: Device) -> Result<Self> {
        let vb = VarBuilder::from_slice_safetensors(model_data, candle::DType::F32, &device)?;
        let model = MobileSAM::new_tiny(vb)?;
        let processor = ImageProcessor::new();

        Ok(Self {
            model,
            processor,
            device,
            current_image_embedding: None,
            img_dims: (1, 1),
        })
    }

    /// 1. 切换图片时调用：生成并缓存 Embedding
    pub fn set_image(&mut self, img: &image::DynamicImage) -> Result<()> {
        // 运行重型编码器
        println!("🚀 正在生成 Image Embedding...");
        // Sam::embeddings 内部会处理 preprocess (缩放和归一化)
        // 但它需要 [3, H, W] 格式且为 F32
        let (input_tensor, w, h) = self.processor.preprocess(img, &self.device)?;
        let embedding = self.model.embeddings(&input_tensor)?;
        self.current_image_embedding = Some(embedding);
        self.img_dims = (w, h);
        println!(
            "✅ Embedding 已就绪: {:.1}x{:.1} (scaled within 1024)",
            w, h
        );

        Ok(())
    }

    /// 2. 用户点击或手势指向时调用：毫秒级生成 Mask
    /// x, y 为归一化坐标 (0.0 - 1.0)
    pub fn get_mask_at(&mut self, x_norm: f32, y_norm: f32) -> Result<Vec<u8>> {
        let embedding = self.current_image_embedding.as_ref().ok_or_else(|| {
            candle::Error::Msg("No image embedding found. Did you call set_image?".to_string())
        })?;

        // 核心修复：点击坐标必须从归一化(0-1)映射到模型的输入空间
        // 我们将输出分辨率设为 256, 256，这要求输入点也按比例缩放到 256 空间内
        let (w, h) = self.img_dims;
        let x_sam = (x_norm * (w as f32 / 4.0)) as f64;
        let y_sam = (y_norm * (h as f32 / 4.0)) as f64;

        let points = &[(x_sam, y_sam, true)];

        // 运行轻量解码器，输出尺寸设为 256，与 FluidEngine 的 1/4 (256) 假设对齐
        let (low_res_mask, _iou) = self.model.forward_for_embeddings(
            embedding, 256, // original_h
            256, // original_w
            points, false, // multimask_output
        )?;

        // 后处理：将 Tensor 转换为一维的字节数组 (0/1 或 0/255)
        self.post_process_mask(low_res_mask)
    }

    fn post_process_mask(&self, mask: Tensor) -> Result<Vec<u8>> {
        // Log raw logit stats
        let min_logit = mask.min_all()?.to_scalar::<f32>()?;
        let max_logit = mask.max_all()?.to_scalar::<f32>()?;
        let mean_logit = mask.mean_all()?.to_scalar::<f32>()?;
        println!(
            "📊 Mask Logits - Min: {:.2}, Max: {:.2}, Mean: {:.2}",
            min_logit, max_logit, mean_logit
        );

        let mask = candle_nn::ops::sigmoid(&mask)?; // [256, 256]

        // Log probability stats
        let min_prob = mask.min_all()?.to_scalar::<f32>()?;
        let max_prob = mask.max_all()?.to_scalar::<f32>()?;
        println!("📊 Mask Probs - Min: {:.2}, Max: {:.2}", min_prob, max_prob);

        let mask = mask.gt(0.5)?; // 阈值判定

        // 扁平化导出给前端或流体引擎
        // 扁平化导出给前端或流体引擎
        let mask = mask.to_dtype(candle::DType::U8)?.flatten_all()?;
        let mask_vec = mask.to_vec1::<u8>()?;

        let count = mask_vec.iter().filter(|&&x| x > 0).count();
        if count == 0 {
            println!("⚠️ Warning: Generated mask is all zeros!");
            // 打印中间值调试
            // let min_val = mask.min(0)?.to_scalar::<f32>()?;
            // let max_val = mask.max(0)?.to_scalar::<f32>()?;
            // println!("Mask value range: {} - {}", min_val, max_val);
        } else {
            println!("✅ Generated mask with {} active pixels", count);
        }

        Ok(mask_vec)
    }
}
