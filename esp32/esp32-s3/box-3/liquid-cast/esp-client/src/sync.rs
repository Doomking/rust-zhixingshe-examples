use core::sync::atomic::{AtomicI32, Ordering};

static DROP_LATE_MS: AtomicI32 = AtomicI32::new(120);
static WAIT_AHEAD_MS: AtomicI32 = AtomicI32::new(40);

pub fn drop_late_ms() -> i32 {
    DROP_LATE_MS.load(Ordering::Relaxed)
}

pub fn wait_ahead_ms() -> i32 {
    WAIT_AHEAD_MS.load(Ordering::Relaxed)
}

pub fn set_params(drop_late_ms: i32, wait_ahead_ms: i32) {
    DROP_LATE_MS.store(drop_late_ms, Ordering::Relaxed);
    WAIT_AHEAD_MS.store(wait_ahead_ms, Ordering::Relaxed);
}

