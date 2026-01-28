use candle::{Device, Result, Tensor};
use image::DynamicImage;

pub struct ImageProcessor {
    pub target_size: u32,
}

impl ImageProcessor {
    pub fn new() -> Self {
        Self { target_size: 1024 }
    }

    pub fn preprocess(&self, img: &DynamicImage, device: &Device) -> Result<(Tensor, u32, u32)> {
        let (old_w, old_h) = (img.width(), img.height());

        // 1. 计算缩放比例，保持长宽比且最大边为 1024
        let scale = self.target_size as f32 / old_w.max(old_h) as f32;
        let new_w = (old_w as f32 * scale) as u32;
        let new_h = (old_h as f32 * scale) as u32;

        let resized = img.resize(new_w, new_h, image::imageops::FilterType::Triangle);
        let actual_w = resized.width();
        let actual_h = resized.height();

        // 2. 创建 1024x1024 的黑色画布并粘贴缩放后的图片 (Padding)
        let mut base = image::RgbImage::new(self.target_size, self.target_size);
        image::imageops::replace(&mut base, &resized.to_rgb8(), 0, 0);
        let data = base.into_raw();

        // 3. 转化为 Tensor [3, 1024, 1024]
        let tensor = Tensor::from_vec(
            data,
            (self.target_size as usize, self.target_size as usize, 3),
            device,
        )?
        .permute((2, 0, 1))?
        .to_dtype(candle::DType::F32)?;

        Ok((tensor, actual_w, actual_h))
    }
}
