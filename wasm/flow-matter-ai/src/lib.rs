mod fluid_engine;
mod image_processor;
mod inference_engine;
mod model;
use candle::Device;
use fluid_engine::FluidEngine;
use image::load_from_memory;
use inference_engine::InferenceEngine;
use model::MaterialProperties;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct InferenceApp {
    engine: InferenceEngine,
    current_img: Option<image::DynamicImage>,
}

#[derive(serde::Serialize)]
pub struct MaskResult {
    pub mask: Vec<u8>,
    pub material: MaterialProperties,
    pub scaled_w: u32,
    pub scaled_h: u32,
}

#[wasm_bindgen]
impl InferenceApp {
    #[wasm_bindgen(constructor)]
    pub fn new(model_data: &[u8]) -> Result<InferenceApp, JsValue> {
        let device = Device::Cpu;
        let engine = InferenceEngine::new(model_data, device)
            .map_err(|e: candle::Error| JsValue::from_str(&e.to_string()))?;
        Ok(Self {
            engine,
            current_img: None,
        })
    }

    pub fn set_image(&mut self, img_bytes: &[u8]) -> Result<(), JsValue> {
        let img = load_from_memory(img_bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.engine
            .set_image(&img)
            .map_err(|e: candle::Error| JsValue::from_str(&e.to_string()))?;
        self.current_img = Some(img);
        Ok(())
    }

    pub fn get_mask_at(&mut self, x_norm: f32, y_norm: f32) -> Result<JsValue, JsValue> {
        let mask = self
            .engine
            .get_mask_at(x_norm, y_norm)
            .map_err(|e: candle::Error| JsValue::from_str(&e.to_string()))?;

        let img = self
            .current_img
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No image"))?;
        let px = (x_norm * img.width() as f32) as u32;
        let py = (y_norm * img.height() as f32) as u32;
        let material = MaterialProperties::from_pixel_at(img, px, py);
        let (scaled_w, scaled_h) = self.engine.img_dims;

        let result = MaskResult {
            mask,
            material,
            scaled_w,
            scaled_h,
        };

        Ok(serde_wasm_bindgen::to_value(&result).unwrap())
    }
}

#[wasm_bindgen]
pub struct PhysicsApp {
    fluid: FluidEngine,
}

#[wasm_bindgen]
impl PhysicsApp {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> PhysicsApp {
        let fluid = FluidEngine::new(width, height);
        Self { fluid }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.fluid.width = width;
        self.fluid.height = height;
    }

    pub fn inject(
        &mut self,
        mask: &[u8],
        img_w: u32,
        img_h: u32,
        scaled_w: u32,
        scaled_h: u32,
        offset_x: f32,
        offset_y: f32,
        unit_scale_x: f32,
        unit_scale_y: f32,
        material: JsValue,
    ) {
        let material: MaterialProperties = serde_wasm_bindgen::from_value(material).unwrap();
        self.fluid.inject_material(
            mask,
            img_w,
            img_h,
            scaled_w,
            scaled_h,
            offset_x,
            offset_y,
            unit_scale_x,
            unit_scale_y,
            material,
        );
    }

    pub fn trigger_collapse(&mut self, avg_audio: f32) {
        self.fluid.apply_collapse(avg_audio);
    }

    pub fn render_frame(&mut self, avg_audio: f32, mouse_x: f32, mouse_y: f32) -> Vec<f32> {
        self.fluid.step(avg_audio, mouse_x, mouse_y);
        self.fluid.get_render_data()
    }

    pub fn update_physics_params(&mut self, viscosity: f32, density: f32) {
        self.fluid.current_material.viscosity = viscosity;
        self.fluid.current_material.density = density;
    }
}
