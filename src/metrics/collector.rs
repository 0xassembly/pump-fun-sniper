use std::sync::Arc;
use anyhow::Result;
use tokio::time::{Duration, interval};
use sysinfo::{System, SystemExt};

use super::types::*;

#[derive(Clone)]
pub struct MetricsCollector {
    sys: Arc<System>,
}

impl MetricsCollector {
    pub fn new() -> Result<Self> {
        let collector = Self {
            sys: Arc::new(System::new_all()),
        };
        
        // Register all metrics
        register_metrics();
        
        Ok(collector)
    }

    pub async fn start_collection(self) {
        let mut interval = interval(Duration::from_secs(15));
        
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                self.collect_system_metrics();
            }
        });
    }

    fn collect_system_metrics(&self) {
        let mut sys = System::new_all();
        sys.refresh_all();

        // Update memory usage
        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        MEMORY_USAGE.set(used_memory as f64);

        // Update thread count (closest equivalent to goroutines)
        let process_count = sys.processes().len();
        GOROUTINE_COUNT.set(process_count as i64);
    }
}
