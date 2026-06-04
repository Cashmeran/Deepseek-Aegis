//! Runtime monitor — timeout kill, OOM detection, orphan reaping.

use aegis_core::error::AgentResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct SandboxMonitor {
    check_interval: Duration,
    running: Arc<AtomicBool>,
}

impl SandboxMonitor {
    pub fn new(check_interval: Duration) -> Self {
        Self {
            check_interval,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self, _pool: Arc<crate::pool::SandboxPool>) -> AgentResult<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(()); // Already running
        }

        let running = Arc::clone(&self.running);
        let interval = self.check_interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                if !running.load(Ordering::SeqCst) { break; }
                // Periodic health check: scan idle instances, kill any that exceed max age
                // For now: placeholder monitoring loop
                tracing::trace!("SandboxMonitor: health check tick");
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
