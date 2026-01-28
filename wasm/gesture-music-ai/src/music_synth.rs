pub struct Synthesizer {
    phase: f64,
}

impl Synthesizer {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
        }
    }

    /// 根据手势坐标生成一帧音频采样数据 (PCM)
    /// x: 0.0 ~ 1.0 (映射到音高)
    /// y: 0.0 ~ 1.0 (映射到滤波或音量)
    pub fn gen_frame(&mut self, x: f32, y: f32) -> Vec<f32> {
        // 1. 将手势 X 映射为频率 (例如 200Hz 到 1000Hz)
        let freq = 200.0 + (x * 800.0);

        // 2. 将手势 Y 映射为音量
        let volume = y * 0.5;

        // 3. 生成音频采样
        let mut samples = Vec::with_capacity(128);
        let sample_rate = 44100.0;

        for _ in 0..128 {
            // 使用简单的正弦波生成
            let val = (self.phase * 2.0 * std::f64::consts::PI).sin();
            samples.push((val as f32) * volume);
            
            // 更新相位
            self.phase += freq as f64 / sample_rate as f64;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }

        samples
    }
}
