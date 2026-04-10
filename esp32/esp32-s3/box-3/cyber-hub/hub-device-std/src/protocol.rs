pub const MAGIC_HEADER: u8 = 0x5A;

// Message Types
pub const MSG_METRICS: u8 = 0x01;    // [Header, 0x01, Len, CPU, MEM]
pub const MSG_FLIP_EVENT: u8 = 0x0F; // [Header, 0x0F, 0, 0]
pub const MSG_VOICE_START: u8 = 0x10; // [Header, 0x10, 0, 0]
pub const MSG_VOICE_DATA: u8 = 0x11;  // [Header, 0x11, LenL, LenH, ...PCM]
pub const MSG_VOICE_END: u8 = 0x12;   // [Header, 0x12, 0, 0]
pub const MSG_FEEDBACK: u8 = 0x20;    // [Header, 0x20, Len, ...Message]
