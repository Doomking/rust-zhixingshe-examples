use wasm_bindgen::prelude::*;
mod fluid_visual;
mod gesture_ai;
mod llm;
mod music_synth;
mod visual_model;

use gesture_ai::Recognizer;

#[wasm_bindgen]
pub struct GestureMusicEngine {
    recognizer: Recognizer,
    music_synth: music_synth::Synthesizer,
    visualizer: fluid_visual::Simulator,
}

#[wasm_bindgen]
impl GestureMusicEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(
        vision_weights: &[u8],
        text_weights: &[u8],
        text_tokenizer: &[u8],
        text_config: &[u8],
    ) -> Result<GestureMusicEngine, JsValue> {
        console_error_panic_hook::set_once();
        let recognizer = Recognizer::new(vision_weights, text_weights, text_tokenizer, text_config)
            .map_err(|e| JsValue::from_str(&format!("Model Init Error: {:?}", e)))?;
        Ok(Self {
            recognizer,
            music_synth: music_synth::Synthesizer::new(),
            visualizer: fluid_visual::Simulator::new(),
        })
    }

    pub fn tick(
        &mut self,
        pixel_ptr: *const u8,
        width: u32,
        height: u32,
    ) -> Result<JsValue, JsValue> {
        // 1. 将指针转换为切片
        let size = (width * height * 4) as usize;
        let pixel_data = unsafe { std::slice::from_raw_parts(pixel_ptr, size) };

        // 2. 预处理图像并获取张量 (在此定义 tensor)
        let tensor = self
            .recognizer
            .preprocess(pixel_data, width, height)
            .map_err(|e| JsValue::from_str(&format!("Preprocess Error: {:?}", e)))?;

        // 3. 推理 (处理 anyhow 错误转换)
        let landmarks = self
            .recognizer
            .inference(&tensor)
            .map_err(|e| JsValue::from_str(&format!("Inference Error: {:?}", e)))?;

        // 4. 提取食指尖 (Landmark 8) 驱动流体和音频
        let index_finger = landmarks[8];
        let audio_frame = self.music_synth.gen_frame(index_finger.x, index_finger.y);
        let visual_data = self.visualizer.update(index_finger.x, index_finger.y);

        // 5. 序列化返回
        Ok(serde_wasm_bindgen::to_value(&(audio_frame, visual_data, landmarks)).unwrap())
    }

    pub fn get_poetic_text(&mut self) -> String {
        let intensity = self.visualizer.get_intensity();

        let mood = if intensity > 0.8 {
            "激昂、雷霆、星火"
        } else {
            "幽静、流云、涟漪"
        };

        let prompt = format!(
            "根据关键词'{}'，生成一句7个字以内、极具诗意的赛博朋克风格短句。",
            mood
        );

        self.recognizer.generate_text(&prompt)
    }
}
