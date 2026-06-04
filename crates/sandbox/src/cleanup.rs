//! Sandbox cleanup — prevent disk leak from abandoned temp directories.

use aegis_core::error::{AgentError, AgentResult};
use std::path::PathBuf;
use std::time::Duration;

pub struct SandboxCleanup {
    temp_root: PathBuf,
    max_age: Duration,
    #[allow(dead_code)]
    max_total_size: u64,
}

#[derive(Debug, Default)]
pub struct CleanupReport {
    pub removed_dirs: usize,
    pub freed_bytes: u64,
}

impl SandboxCleanup {
    pub fn new(temp_root: PathBuf, max_age: Duration) -> Self {
        Self { temp_root, max_age, max_total_size: 5 * 1024 * 1024 * 1024 }
    }

    pub fn scan_and_prune(&self) -> AgentResult<CleanupReport> {
        if !self.temp_root.exists() { return Ok(CleanupReport::default()); }

        let mut report = CleanupReport::default();
        for entry in std::fs::read_dir(&self.temp_root).map_err(|e| AgentError::Internal(format!("read_dir: {}", e)))? {
            let entry = entry.map_err(|e| AgentError::Internal(format!("entry: {}", e)))?;
            let path = entry.path();
            if !path.is_dir() { continue; }

            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified.elapsed().unwrap_or_default() > self.max_age {
                        let size = dir_size(&path);
                        std::fs::remove_dir_all(&path).ok();
                        report.removed_dirs += 1;
                        report.freed_bytes += size;
                    }
                }
            }
        }
        Ok(report)
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prune_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cleanup = SandboxCleanup::new(dir.path().to_path_buf(), Duration::from_secs(0));
        let report = cleanup.scan_and_prune().unwrap();
        assert_eq!(report.removed_dirs, 0);
    }
}
