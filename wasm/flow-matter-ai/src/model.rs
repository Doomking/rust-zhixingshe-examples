pub use candle_transformers::models::segment_anything::sam::Sam as MobileSAM;

// --- 5. 物质属性定义 (Material Properties) ---
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialProperties {
    pub label: String,       // 物质名称
    pub base_frequency: f32, // 基础音频频率 (Hz)
    pub viscosity: f32,      // 流体粘度 (0.0 - 1.0)
    pub tension: f32,        // 表面张力
    pub density: f32,        // 粒子生成密度
    pub color_rgb: (u8, u8, u8),
    pub hue: f32,
}

impl Default for MaterialProperties {
    fn default() -> Self {
        Self {
            label: "unknown".to_string(),
            base_frequency: 440.0,
            viscosity: 0.1,
            tension: 0.5,
            density: 1.0,
            color_rgb: (255, 255, 255),
            hue: 0.0,
        }
    }
}

impl MaterialProperties {
    /// 根据点击位置的颜色和亮度简单判断物质属性
    pub fn from_pixel_at(img: &image::DynamicImage, _x: u32, _y: u32) -> Self {
        use image::GenericImageView;
        if _x >= img.width() || _y >= img.height() {
            return Self::default();
        }

        let pixel = img.get_pixel(_x, _y);
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];

        // 计算 HSL 基础信息用于合成
        let (h, s, l) = rgb_to_hsl(r, g, b);
        let brightness = l;

        // 默认属性
        let mut props = Self {
            label: "Fluid".to_string(),
            base_frequency: 200.0 + h,        // 色相决定基频
            viscosity: 0.1 + (1.0 - s) * 0.5, // 饱和度低则粘稠
            tension: 0.5,
            density: 1.0 + s * 0.5,
            color_rgb: (r, g, b),
            hue: h,
        };

        // 命名分类
        if brightness > 0.85 {
            props.label = "Super White: Light/Energy".to_string();
            props.base_frequency = 1200.0;
        } else if brightness < 0.15 {
            props.label = "Deep Black: Void/Heavy".to_string();
            props.base_frequency = 60.0;
            props.viscosity = 0.8;
        } else if h < 30.0 || h > 330.0 {
            props.label = "Red/Warm: Flame/Passion".to_string();
        } else if h > 90.0 && h < 160.0 {
            props.label = "Green: Organic/Life".to_string();
        } else if h > 190.0 && h < 260.0 {
            props.label = "Blue: Water/Deep".to_string();
        } else if h > 45.0 && h < 75.0 {
            props.label = "Yellow: Electric/Sun".to_string();
        }

        props
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if max == min {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if max == r {
        (g - b) / d + (if g < b { 6.0 } else { 0.0 })
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    (h * 60.0, s, l)
}
