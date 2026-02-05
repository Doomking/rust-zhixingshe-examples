use crate::schedulers::{EulerDiscreteScheduler, EulerDiscreteSchedulerConfig};
use crate::translator::Translator;
use anyhow::{Error, Result};
use candle_core::{DType, Device, IndexOp, Module, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::stable_diffusion::schedulers::{PredictionType, Scheduler};
use candle_transformers::models::stable_diffusion::{
    clip::ClipTextTransformer, unet_2d::UNet2DConditionModel, vae::AutoEncoderKL,
};
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

// 引入配置定义，为了避免命名冲突使用别名 (Import Config definitions)
use candle_transformers::models::stable_diffusion::clip::Config as ClipConfig;
use candle_transformers::models::stable_diffusion::unet_2d::UNet2DConditionModelConfig as UNetConfig;
use candle_transformers::models::stable_diffusion::vae::AutoEncoderKLConfig as VaeConfig;

pub struct StableDiffusion {
    clip: ClipTextTransformer,
    vae: AutoEncoderKL,
    unet: UNet2DConditionModel,
    tokenizer: Tokenizer,
    scheduler: EulerDiscreteScheduler, // 使用 Euler 调度器
    device: Device,
    translator: Arc<Mutex<Translator>>,
}

impl StableDiffusion {
    pub fn new(device: &Device) -> Result<Self> {
        let models_dir = std::path::Path::new("models/sd-1.5");
        if !models_dir.exists() {
            std::fs::create_dir_all(models_dir)?;
        }

        let files = vec![
            (
                "https://hf-mirror.com/runwayml/stable-diffusion-v1-5/resolve/main/text_encoder/model.safetensors",
                "text_encoder.safetensors",
            ),
            (
                "https://hf-mirror.com/runwayml/stable-diffusion-v1-5/resolve/main/vae/diffusion_pytorch_model.safetensors",
                "vae.safetensors",
            ),
            (
                "https://hf-mirror.com/runwayml/stable-diffusion-v1-5/resolve/main/unet/diffusion_pytorch_model.safetensors",
                "unet.safetensors",
            ),
            (
                "https://hf-mirror.com/openai/clip-vit-large-patch14/resolve/main/tokenizer.json",
                "tokenizer.json",
            ),
        ];

        let mut paths = std::collections::HashMap::new();

        for (url, filename) in files {
            let local_path = models_dir.join(filename);
            paths.insert(filename, local_path.clone());

            if !local_path.exists() {
                println!("Downloading {} to {:?}...", filename, local_path);
                let status = std::process::Command::new("curl")
                    .arg("-L")
                    .arg(url)
                    .arg("-o")
                    .arg(&local_path)
                    .status()?;

                if !status.success() {
                    return Err(Error::msg(format!("Failed to download {}", filename)));
                }
            }
        }

        // 1. 获取本地路径
        let clip_weights = paths.get("text_encoder.safetensors").unwrap();
        let vae_weights = paths.get("vae.safetensors").unwrap();
        let unet_weights = paths.get("unet.safetensors").unwrap();
        let tokenizer_filename = paths.get("tokenizer.json").unwrap();

        let mut tokenizer = Tokenizer::from_file(tokenizer_filename).map_err(Error::msg)?;

        // Configure Padding to fixed length 77 (CLIP standard)
        // SD 1.5 Tokenizer (OpenAI CLIP) usually uses EOS token as padding.
        // Pad ID 49407 is standard for this tokenizer.
        let pad_id = 49407;
        let padding = tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::Fixed(77),
            pad_id,
            ..Default::default()
        };
        tokenizer.with_padding(Some(padding));

        // Enforce Truncation to 77 tokens
        let truncation = tokenizers::TruncationParams {
            max_length: 77,
            strategy: tokenizers::TruncationStrategy::LongestFirst,
            ..Default::default()
        };
        tokenizer.with_truncation(Some(truncation));

        // 2. 加载配置 (Load Configs)
        let clip_config = ClipConfig::v1_5();

        // correct VAE Config for SD 1.5
        let vae_config = VaeConfig {
            block_out_channels: vec![128, 256, 512, 512],
            latent_channels: 4,
            layers_per_block: 2,
            norm_num_groups: 32,
            ..Default::default()
        };

        // Correct UNet Config for SD 1.5
        // unet_config.json says: cross_attention_dim: 768
        let mut unet_config = UNetConfig::default();
        unet_config.cross_attention_dim = 768;

        let scheduler_config = EulerDiscreteSchedulerConfig {
            prediction_type: PredictionType::Epsilon,
            ..Default::default()
        };

        // 3. 加载模型 (Load Models)
        // Switch back to passed device (Metal/CUDA) for speed
        let dtype = DType::F32;
        // let device = &Device::Cpu; // Removed CPU force override

        let clip = ClipTextTransformer::new(
            unsafe { VarBuilder::from_mmaped_safetensors(&[clip_weights], dtype, device)? },
            &clip_config,
        )?;

        let vae = AutoEncoderKL::new(
            unsafe { VarBuilder::from_mmaped_safetensors(&[vae_weights], dtype, &Device::Cpu)? },
            3,
            3,
            vae_config,
        )?;

        let unet = UNet2DConditionModel::new(
            unsafe { VarBuilder::from_mmaped_safetensors(&[unet_weights], dtype, device)? },
            4,     // in_channels
            4,     // out_channels
            false, // use_flash_attn
            unet_config,
        )?;

        // 初始化调度器 (Initialize Scheduler)
        let scheduler = EulerDiscreteScheduler::new(scheduler_config)?;

        // Initialize Translator on CPU (Marian is small, CPU is fine and safer for memory)
        // or usage same device? Marian is small.
        // Let's use the passed device.
        let translator = Translator::new(device)?;
        println!("Translator initialized.");

        Ok(Self {
            clip,
            vae,
            unet,
            tokenizer,
            scheduler,
            device: device.clone(),
            translator: Arc::new(Mutex::new(translator)),
        })
    }

    /// 执行文生图任务
    pub fn generate(
        &self,
        original_prompt: &str,
        n_steps: usize,
        guidance_scale: f64,
        seed: Option<u64>,
    ) -> Result<image::DynamicImage> {
        // 0. Neural Translation (Chinese -> English)
        let mut prompt = original_prompt.to_string();

        // Attempt translation
        match self.translator.lock().unwrap().translate(original_prompt) {
            Ok(translated) => {
                prompt = translated;
            }
            Err(e) => {
                println!("Translation failed: {}, utilizing original prompt", e);
            }
        }

        // Force Anime Style
        let style_suffix = ", Studio Ghibli style, Hayao Miyazaki, pastel colors, bright sky, hand-drawn style, cheerful atmosphere, soft lighting, cel shaded, warm tones, masterpiece, best quality";
        prompt.push_str(style_suffix);

        println!(
            "开始生成 (Starting Generation): Original='{}' -> Processed='{}', Steps={}, Scale={}",
            original_prompt, prompt, n_steps, guidance_scale
        );

        // 1. 文本编码 (Text Encoding)
        let tokens = self.tokenizer.encode(prompt, true).map_err(Error::msg)?;
        let tokens = Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?;
        let text_embeddings = self.clip.forward(&tokens)?;

        // 无条件 Embedding (Unconditional Embedding)
        let uncond_tokens = self.tokenizer.encode("", true).map_err(Error::msg)?;
        let uncond_tokens = Tensor::new(uncond_tokens.get_ids(), &self.device)?.unsqueeze(0)?;
        let uncond_embeddings = self.clip.forward(&uncond_tokens)?;

        let text_embeddings = Tensor::cat(&[&uncond_embeddings, &text_embeddings], 0)?;

        // 2. 初始化潜在空间噪声 (Latent Noise Initialization) [1, 4, 64, 64]
        // Use F32
        let latents = if let Some(s) = seed {
            use rand::rngs::StdRng;
            use rand::{Rng, SeedableRng};
            println!("🌱 Generating latents with seed: {}", s);
            let mut rng = StdRng::seed_from_u64(s);
            let total_elements = 1 * 4 * 64 * 64;
            let mut data = vec![0f32; total_elements];
            // Fill with standard normal distribution
            for x in data.iter_mut() {
                *x = rng.sample(rand_distr::StandardNormal);
            }
            Tensor::from_vec(data, (1, 4, 64, 64), &self.device)?.to_dtype(DType::F32)?
        } else {
            Tensor::randn(0f32, 1f32, (1, 4, 64, 64), &self.device)?.to_dtype(DType::F32)?
        };

        let mut scheduler = self.scheduler.clone();
        scheduler.set_timesteps(n_steps)?;
        let timesteps = scheduler.timesteps().to_vec();

        // Fix: Scale the initial noise by the scheduler's sigma
        // This is crucial for EulerDiscreteScheduler to work correctly with SD 1.5
        let latents = (latents * scheduler.init_noise_sigma())?;

        let mut latents = latents;

        // 3. 去噪循环 (Denoising Loop)
        for (step_index, &timestep) in timesteps.iter().enumerate() {
            println!("Step {}/{}", step_index + 1, n_steps);

            let latent_model_input = Tensor::cat(&[&latents, &latents], 0)?;
            let latent_model_input = scheduler.scale_model_input(latent_model_input, timestep)?;

            // UNet forward
            let noise_pred =
                self.unet
                    .forward(&latent_model_input, timestep as f64, &text_embeddings)?;

            let noise_pred_chunks = noise_pred.chunk(2, 0)?;
            let (noise_pred_uncond, noise_pred_text) =
                (&noise_pred_chunks[0], &noise_pred_chunks[1]);

            // Classifier-Free Guidance
            let noise_pred =
                (noise_pred_uncond + ((noise_pred_text - noise_pred_uncond)? * guidance_scale)?)?;

            // Scheduler Step
            latents = scheduler.step(&noise_pred, timestep, &latents)?; // Update latents directly
        }

        // 4. VAE 解码 (VAE Decoding)
        println!("解码潜在变量 (Decoding Latents) on CPU...");
        // Move latents to CPU for VAE decoding to avoid Metal issues
        let latents = latents.to_device(&Device::Cpu)?;
        let latents = (latents / 0.18215)?;
        let image = self.vae.decode(&latents)?;

        // 5. 图像后处理 (Post-processing)
        // [1, 3, 512, 512] -> [3, 512, 512] -> [512, 512, 3] -> U8
        // 处理 Result 嵌套: (image / 2.)?.add_scalar(0.5)?
        let image = (image / 2.)?.affine(1.0, 0.5)?;
        let image = image.clamp(0f32, 1f32)?;
        let image = (image * 255.0)?.to_dtype(DType::U8)?;
        let image = image.i(0)?;
        let image = image.permute((1, 2, 0))?;

        let (width, height) = (image.dim(0)?, image.dim(1)?);
        let raw_data = image.flatten_all()?.to_vec1::<u8>()?;

        let img_buffer = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
            width as u32,
            height as u32,
            raw_data,
        )
        .ok_or(Error::msg("Failed to create image buffer"))?;

        Ok(image::DynamicImage::ImageRgb8(img_buffer))
    }
}
