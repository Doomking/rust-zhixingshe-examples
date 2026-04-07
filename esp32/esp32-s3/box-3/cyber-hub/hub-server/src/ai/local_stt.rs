use anyhow::{Result, Context};
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
use std::path::Path;
use tracing::{info, warn, error, debug};

use std::ffi::{c_char, c_void, CStr};

pub struct LocalStt {
    ctx: WhisperContext,
}

// 必须是 extern "C" 类型的纯函数，不能是闭包
unsafe extern "C" fn whisper_log_callback(_level: u32, _msg: *const c_char, _user_data: *mut c_void) {
    // 保持静默，不进行任何操作
}

impl LocalStt {
    pub fn new(model_path: &str) -> Result<Self> {
        info!("[LocalSTT] Loading model from {}...", model_path);
        if !Path::new(model_path).exists() {
            return Err(anyhow::anyhow!("Model file not found at {}. Please download ggml-base.bin first.", model_path));
        }

        // 屏蔽底层 whisper.cpp 的刷屏日志
        unsafe {
            whisper_rs::set_log_callback(Some(whisper_log_callback), std::ptr::null_mut());
        }

        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .context("Failed to load Whisper model")?;

        Ok(Self { ctx })
    }

    pub fn transcribe(&self, pcm_data: &[i16]) -> Result<String> {
        // 1. 双声道合一并且转换格式 (Stereo -> Mono, i16 -> f32)
        // 注意：底层采集的是立体声(2ch)，Whisper 要求单声道(1ch)
        let mut f32_samples = Vec::with_capacity(pcm_data.len() / 2);
        for chunk in pcm_data.chunks_exact(2) {
            let mono = (chunk[0] as f32 + chunk[1] as f32) / 2.0;
            // 2.5x 软件数字增益 (平和放大，减少削波失真)
            f32_samples.push(((mono / 32768.0) * 2.5).clamp(-1.0, 1.0));
        }

        // 2. 创建推理状态
        let mut state = self.ctx.create_state().context("Failed to create whisper state")?;
        
        // 3. 配置参数 (回归标准束搜索 Beam-7 - 在稳定性与深层纠错间取得平衡)
        let mut params = FullParams::new(SamplingStrategy::BeamSearch { 
            beam_size: 7, 
            patience: 1.0 
        });
        
        params.set_n_threads(4);
        params.set_language(Some("zh"));
        
        // 核心优化：彻底中性化。不带任何场景和内容偏见，仅告知模型语言类型以确保通用识别精度。
        params.set_initial_prompt("这是一段中文语音录音。"); 
        
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_suppress_blank(false);
        // 核心优化：斩断重复循环。模型在推理当前切片时不再参考上一段内容，彻底解决“鸽子鸽子”现象。
        params.set_no_context(true);

        // 4. 执行推理
        state.full(params, &f32_samples).context("Failed to run whisper inference")?;

        // 5. 提取文本并过滤幻觉
        let mut result = String::new();
        for segment in state.as_iter() {
            // 误触发极致放宽：如果模型认为这是噪音的概率 > 80%，则丢弃 (救回被误杀的“词语表”)
            let no_speech = segment.no_speech_probability();
            if no_speech > 0.8 {
                debug!("[STT] High no_speech_prob ({:.2}). Skipping.", no_speech);
                continue;
            }

            if let Ok(text) = segment.to_str_lossy() {
                result.push_str(text.trim());
            }
        }

        // 6. 深度清洗逻辑 (V15：无视长度的括号剥离 + 强制黑名单)
        let mut finalized = result.trim().to_string();
        
        // A. 拦截显而易见的废话关键词 (YouTube/字幕组幻觉 + 提示词回响)
        let hard_blacklist = [
            "字幕君", "点赞", "订阅", "转发", "不懂", "看不懂", "收看", "关注",
            // Whisper prompt-echo：模型在无有效语音时会把 initial_prompt 原文吐出来
            "这是一段中文语音录音",
        ];
        for bad_word in hard_blacklist {
            if finalized.contains(bad_word) {
                debug!("[STT] Hard blacklist suppressed: \"{}\"", finalized);
                return Ok(String::new());
            }
        }

        // B. 彻底剥离所有括号内内容 (无论长短)
        fn strip_brackets(s: &str) -> String {
            let mut out = String::new();
            let mut depth = 0;
            for c in s.chars() {
                match c {
                    '(' | '（' | '[' | '【' => depth += 1,
                    ')' | '）' | ']' | '】' => if depth > 0 { depth -= 1 },
                    _ if depth == 0 => out.push(c),
                    _ => {}
                }
            }
            out.trim().to_string()
        }
        
        finalized = strip_brackets(&finalized);

        // C. 有效荷载检查：如果剥离后只剩下标点或空空如也，判定为幻觉
        let pure_text: String = finalized.chars().filter(|c| c.is_alphanumeric()).collect();
        if pure_text.is_empty() {
             debug!("[STT] No meaningful payload after cleaning. Returning empty.");
             return Ok(String::new());
        }

        // D. 简单的相邻重复项去重 (补充防御：解决“鸽子鸽子”重复输出问题)
        let chars: Vec<char> = finalized.chars().collect();
        if chars.len() >= 2 {
             let mid = chars.len() / 2;
             if chars.len() % 2 == 0 && chars[0..mid] == chars[mid..] {
                 let deduplicated: String = chars[0..mid].iter().collect();
                 debug!("[STT] Deduplicated loop: \"{}\" -> \"{}\"", finalized, deduplicated);
                 finalized = deduplicated;
             }
        }

        Ok(finalized)
    }
}
