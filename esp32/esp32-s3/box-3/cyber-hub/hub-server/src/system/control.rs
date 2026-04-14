use tracing::{error, info, warn};
use std::process::Command;

// ── 锁屏 ────────────────────────────────────────────────────────────────────

pub fn trigger_lock() {
    #[cfg(target_os = "macos")]
    {
        trigger_macos_lock_inner();
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if try_run_cmd("loginctl", &["lock-session"]) {
            return;
        }
        warn!("[LOCK] linux lock command not available");
        return;
    }
    #[cfg(target_os = "windows")]
    {
        if try_run_cmd("rundll32.exe", &["user32.dll,LockWorkStation"]) {
            return;
        }
        warn!("[LOCK] windows lock command not available");
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        warn!("[LOCK] unsupported platform");
    }
}

pub fn trigger_macos_lock() {
    trigger_lock();
}

#[cfg(target_os = "macos")]
fn trigger_macos_lock_inner() {
    info!("[COMMAND] Lock Screen triggered!");

    let private_api_success = unsafe {
        match libloading::Library::new(
            "/System/Library/PrivateFrameworks/login.framework/Versions/Current/login",
        ) {
            Ok(lib) => match lib.get::<unsafe extern "C" fn()>(b"SACLockScreenImmediate") {
                Ok(lock_func) => {
                    info!("[LOCK] Calling SACLockScreenImmediate via Private API...");
                    lock_func();
                    true
                }
                Err(e) => {
                    warn!("[LOCK] Symbol SACLockScreenImmediate not found: {}", e);
                    false
                }
            },
            Err(e) => {
                warn!("[LOCK] Could not load login.framework: {}", e);
                false
            }
        }
    };

    if !private_api_success {
        info!("[LOCK] Private API Lock failed, trying fallback (pmset)...");
        if let Err(e) = Command::new("pmset").arg("displaysleepnow").spawn() {
            error!("Failed to execute pmset: {}", e);
        }
    }
}

// ── 音量控制 ─────────────────────────────────────────────────────────────────

/// 音量调大（+10，上限 100）
pub fn volume_up() {
    info!("[COMMAND] Volume Up triggered!");
    #[cfg(target_os = "macos")]
    {
        run_applescript_lines(&[
            "set curVol to output volume of (get volume settings)",
            "set newVol to curVol + 10",
            "if newVol > 100 then set newVol to 100",
            "set volume output volume newVol",
        ]);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if try_run_cmd("pactl", &["set-sink-volume", "@DEFAULT_SINK@", "+10%"]) {
            return;
        }
        if try_run_cmd("amixer", &["-D", "pulse", "sset", "Master", "10%+"]) {
            return;
        }
        warn!("[VOLUME] linux volume up command not available");
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    warn!("[VOLUME] unsupported platform for volume up");
}

/// 音量调小（-10，下限 0）
pub fn volume_down() {
    info!("[COMMAND] Volume Down triggered!");
    #[cfg(target_os = "macos")]
    {
        run_applescript_lines(&[
            "set curVol to output volume of (get volume settings)",
            "set newVol to curVol - 10",
            "if newVol < 0 then set newVol to 0",
            "set volume output volume newVol",
        ]);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if try_run_cmd("pactl", &["set-sink-volume", "@DEFAULT_SINK@", "-10%"]) {
            return;
        }
        if try_run_cmd("amixer", &["-D", "pulse", "sset", "Master", "10%-"]) {
            return;
        }
        warn!("[VOLUME] linux volume down command not available");
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    warn!("[VOLUME] unsupported platform for volume down");
}

/// 静音
pub fn mute() {
    info!("[COMMAND] Mute triggered!");
    #[cfg(target_os = "macos")]
    {
        run_applescript("set volume with output muted");
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if try_run_cmd("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "1"]) {
            return;
        }
        if try_run_cmd("amixer", &["-D", "pulse", "sset", "Master", "mute"]) {
            return;
        }
        warn!("[VOLUME] linux mute command not available");
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    warn!("[VOLUME] unsupported platform for mute");
}

/// 取消静音
pub fn unmute() {
    info!("[COMMAND] Unmute triggered!");
    #[cfg(target_os = "macos")]
    {
        run_applescript("set volume without output muted");
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if try_run_cmd("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]) {
            return;
        }
        if try_run_cmd("amixer", &["-D", "pulse", "sset", "Master", "unmute"]) {
            return;
        }
        warn!("[VOLUME] linux unmute command not available");
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    warn!("[VOLUME] unsupported platform for unmute");
}

// ── 内部工具 ─────────────────────────────────────────────────────────────────

/// 执行单行 AppleScript
fn run_applescript(script: &str) {
    run_applescript_lines(&[script]);
}

/// 执行多行 AppleScript（每行一个 -e 参数，macOS 标准做法）
fn run_applescript_lines(lines: &[&str]) {
    let mut cmd = Command::new("osascript");
    for line in lines {
        cmd.arg("-e").arg(line);
    }
    match cmd.output() {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            error!("[CONTROL] osascript error: {}", stderr.trim());
        }
        Err(e) => error!("[CONTROL] osascript failed to spawn: {}", e),
        _ => {}
    }
}

fn try_run_cmd(bin: &str, args: &[&str]) -> bool {
    match Command::new(bin).args(args).output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!("[CONTROL] {} {:?} failed: {}", bin, args, stderr.trim());
            false
        }
        Err(_) => false,
    }
}
