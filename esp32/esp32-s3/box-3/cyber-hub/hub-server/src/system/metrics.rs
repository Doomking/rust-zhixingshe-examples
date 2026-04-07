use sysinfo::{System, CpuRefreshKind, MemoryRefreshKind, RefreshKind};
use tokio::sync::Mutex;
use std::sync::Arc;

pub struct MetricsMonitor {
    sys: Arc<Mutex<System>>,
}

impl MetricsMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();
        Self {
            sys: Arc::new(Mutex::new(sys)),
        }
    }

    pub fn get_sys_ref(&self) -> Arc<Mutex<System>> {
        self.sys.clone()
    }

    pub async fn get_usage(&self) -> (u8, u8) {
        let mut s = self.sys.lock().await;
        s.refresh_all();
        
        let cpu = s.global_cpu_usage() as u8;
        let mem = (s.used_memory() as f64 / s.total_memory() as f64 * 100.0) as u8;
        (cpu, mem)
    }
}
