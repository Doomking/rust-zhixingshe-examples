//! Downlink queue: mono s16le 16 kHz → stereo I2S buffer.

use log::*;
use std::collections::VecDeque;
use std::sync::mpsc;

pub(crate) struct MonoPlayback {
    rx: mpsc::Receiver<Vec<u8>>,
    queue: VecDeque<Vec<u8>>,
    cur: Vec<u8>,
    off: usize,
}

impl MonoPlayback {
    pub(crate) fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            queue: VecDeque::new(),
            cur: Vec::new(),
            off: 0,
        }
    }

    fn poll_rx(&mut self) {
        while let Ok(v) = self.rx.try_recv() {
            if !v.is_empty() {
                debug!("[PLAYBACK] queued {} bytes", v.len());
                self.queue.push_back(v);
            }
        }
    }

    fn advance_buffer(&mut self) {
        if self.off >= self.cur.len() {
            self.cur = self.queue.pop_front().unwrap_or_default();
            self.off = 0;
        }
    }

    /// 输出 `frames` 个立体声帧（s16le 交错 L,R），与 `read_len_bytes` 一致。
    pub(crate) fn fill_stereo_s16le(&mut self, dst: &mut [u8], frames: usize) {
        // Boost queued mono PCM (wake/done); levels from `say`+ffmpeg are usually well below full scale.
        const PLAYBACK_GAIN: i32 = 3;
        self.poll_rx();
        for i in 0..frames {
            self.advance_buffer();
            let m = if self.off + 2 <= self.cur.len() {
                let raw = i16::from_le_bytes([self.cur[self.off], self.cur[self.off + 1]]) as i32;
                self.off += 2;
                (raw * PLAYBACK_GAIN).clamp(i16::MIN as i32, i16::MAX as i32) as i16
            } else {
                0i16
            };
            let b = m.to_le_bytes();
            let idx = i * 4;
            dst[idx..idx + 2].copy_from_slice(&b);
            dst[idx + 2..idx + 4].copy_from_slice(&b);
        }
    }
}
