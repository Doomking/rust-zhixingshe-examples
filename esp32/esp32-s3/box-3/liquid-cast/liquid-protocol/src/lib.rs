#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    VideoJpeg = 0x01,
    AudioPcm = 0x02,

    // Control plane (P0 blueprint upgrade)
    ControlHello = 0x10,
    ControlAck = 0x11,
    ControlPing = 0x12,

    Unknown = 0xFF,
}

impl From<u8> for FrameType {
    fn from(val: u8) -> Self {
        match val {
            0x01 => FrameType::VideoJpeg,
            0x02 => FrameType::AudioPcm,
            0x10 => FrameType::ControlHello,
            0x11 => FrameType::ControlAck,
            0x12 => FrameType::ControlPing,
            _ => FrameType::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrameHeader {
    pub frame_type: FrameType,
    pub timestamp_ms: u32,
    pub payload_len: u32,
}

impl FrameHeader {
    pub const SIZE: usize = 12;
    pub const MAGIC: [u8; 2] = [0x4C, 0x43]; // "LC"

    pub fn serialize(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..2].copy_from_slice(&Self::MAGIC);
        buf[2] = self.frame_type as u8;
        buf[3..7].copy_from_slice(&self.timestamp_ms.to_be_bytes());
        buf[7..11].copy_from_slice(&self.payload_len.to_be_bytes());

        let mut checksum = 0u8;
        for i in 0..11 {
            checksum ^= buf[i];
        }
        buf[11] = checksum;
        buf
    }

    pub fn deserialize(buf: &[u8; 12]) -> Option<Self> {
        if buf[0] != Self::MAGIC[0] || buf[1] != Self::MAGIC[1] {
            return None;
        }

        let mut checksum = 0u8;
        for i in 0..11 {
            checksum ^= buf[i];
        }
        if checksum != buf[11] {
            return None;
        }

        let frame_type = FrameType::from(buf[2]);
        let timestamp_ms = u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]);
        let payload_len = u32::from_be_bytes([buf[7], buf[8], buf[9], buf[10]]);

        Some(Self {
            frame_type,
            timestamp_ms,
            payload_len,
        })
    }
}

// ---------------------------
// Control plane payloads
// ---------------------------

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub struct AvSyncParams {
    pub drop_late_ms: i16,
    pub wait_ahead_ms: i16,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaParams {
    pub video_w: u16,
    pub video_h: u16,
    pub video_fps: u16,
    pub jpeg_q: u8,
    pub audio_sample_rate: u32,
    pub audio_chunk_bytes: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlHello {
    pub version: u16,
    pub media: MediaParams,
    pub av: AvSyncParams,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlAck {
    pub version: u16,
    pub media: MediaParams,
    pub av: AvSyncParams,
}

// Binary layout (big-endian):
// 0..4  magic "LCH1"
// 4..6  version u16
// 6..8  video_w u16
// 8..10 video_h u16
// 10..12 video_fps u16
// 12    jpeg_q u8
// 13..17 audio_sample_rate u32
// 17..19 audio_chunk_bytes u16
// 19..21 drop_late_ms i16
// 21..23 wait_ahead_ms i16
pub const CONTROL_PAYLOAD_LEN: usize = 23;

fn magic_ok(b: &[u8]) -> bool {
    b.len() >= 4 && &b[0..4] == b"LCH1"
}

impl ControlHello {
    pub fn serialize(&self) -> [u8; CONTROL_PAYLOAD_LEN] {
        let mut b = [0u8; CONTROL_PAYLOAD_LEN];
        b[0..4].copy_from_slice(b"LCH1");
        b[4..6].copy_from_slice(&self.version.to_be_bytes());
        b[6..8].copy_from_slice(&self.media.video_w.to_be_bytes());
        b[8..10].copy_from_slice(&self.media.video_h.to_be_bytes());
        b[10..12].copy_from_slice(&self.media.video_fps.to_be_bytes());
        b[12] = self.media.jpeg_q;
        b[13..17].copy_from_slice(&self.media.audio_sample_rate.to_be_bytes());
        b[17..19].copy_from_slice(&self.media.audio_chunk_bytes.to_be_bytes());
        b[19..21].copy_from_slice(&self.av.drop_late_ms.to_be_bytes());
        b[21..23].copy_from_slice(&self.av.wait_ahead_ms.to_be_bytes());
        b
    }

    pub fn deserialize(payload: &[u8]) -> Option<Self> {
        if payload.len() != CONTROL_PAYLOAD_LEN || !magic_ok(payload) {
            return None;
        }
        let version = u16::from_be_bytes([payload[4], payload[5]]);
        let media = MediaParams {
            video_w: u16::from_be_bytes([payload[6], payload[7]]),
            video_h: u16::from_be_bytes([payload[8], payload[9]]),
            video_fps: u16::from_be_bytes([payload[10], payload[11]]),
            jpeg_q: payload[12],
            audio_sample_rate: u32::from_be_bytes([payload[13], payload[14], payload[15], payload[16]]),
            audio_chunk_bytes: u16::from_be_bytes([payload[17], payload[18]]),
        };
        let av = AvSyncParams {
            drop_late_ms: i16::from_be_bytes([payload[19], payload[20]]),
            wait_ahead_ms: i16::from_be_bytes([payload[21], payload[22]]),
        };
        Some(Self { version, media, av })
    }
}

impl ControlAck {
    pub fn serialize(&self) -> [u8; CONTROL_PAYLOAD_LEN] {
        ControlHello {
            version: self.version,
            media: self.media,
            av: self.av,
        }
        .serialize()
    }

    pub fn deserialize(payload: &[u8]) -> Option<Self> {
        ControlHello::deserialize(payload).map(|h| Self {
            version: h.version,
            media: h.media,
            av: h.av,
        })
    }
}

