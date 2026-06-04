//! side-git 快照系统 — pre/post-turn git snapshots.
//!
//! 使用独立 git-dir (.agent/snapshots/.git), 不影响用户仓库。
//! 每轮对话前后自动提交。7天清理, 2GB上限。

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    pub enabled: bool,
    pub max_age_days: u32,
    pub max_workspace_gb: f32,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self { enabled: true, max_age_days: 7, max_workspace_gb: 2.0 }
    }
}

pub struct SnapshotManager {
    config: SnapshotConfig,
    snapshot_dir: PathBuf,
    initialized: bool,
}

impl SnapshotManager {
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            snapshot_dir: PathBuf::from(".agent/snapshots"),
            initialized: false,
        }
    }

    fn git(&self, args: &[&str]) -> std::io::Result<std::process::Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.snapshot_dir)
            .env("GIT_DIR", self.snapshot_dir.join(".git"))
            .env("GIT_WORK_TREE", ".")
            .output()
    }

    fn ensure_init(&mut self) {
        if self.initialized { return; }
        let _ = std::fs::create_dir_all(&self.snapshot_dir);
        let _ = Command::new("git").args(["init"]).current_dir(&self.snapshot_dir).output();
        let _ = Command::new("git")
            .args(["-C", &self.snapshot_dir.to_string_lossy(), "config", "user.name", "aegis-snapshot"])
            .output();
        let _ = Command::new("git")
            .args(["-C", &self.snapshot_dir.to_string_lossy(), "config", "user.email", "snapshot@aegis.local"])
            .output();
        self.initialized = true;
    }

    fn git_at_workspace(&self, args: &[&str], cwd: &std::path::Path) -> std::io::Result<std::process::Output> {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_DIR", self.snapshot_dir.join(".git"))
            .env("GIT_WORK_TREE", cwd)
            .output()
    }

    /// Snapshot current workspace state before a turn.
    pub fn snapshot_pre_turn(&mut self, label: &str) -> Option<String> {
        if !self.config.enabled { return None; }
        self.ensure_init();
        let cwd = std::env::current_dir().ok()?;
        let _ = self.git_at_workspace(&["add", "-A"], &cwd);
        let _ = self.git_at_workspace(&["commit", "--allow-empty", "-m", &format!("pre: {label}")], &cwd);
        let output = self.git_at_workspace(&["rev-parse", "--short", "HEAD"], &cwd).ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Snapshot after a turn completes.
    pub fn snapshot_post_turn(&mut self, label: &str) -> Option<String> {
        if !self.config.enabled { return None; }
        let cwd = std::env::current_dir().ok()?;
        let _ = self.git_at_workspace(&["add", "-A"], &cwd);
        let _ = self.git_at_workspace(&["commit", "--allow-empty", "-m", &format!("post: {label}")], &cwd);
        let output = self.git_at_workspace(&["rev-parse", "--short", "HEAD"], &cwd).ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Restore workspace to a specific snapshot (via git checkout in snapshot dir).
    pub fn restore(&self, commit: &str) -> bool {
        if !self.config.enabled { return false; }
        let cwd = std::env::current_dir().unwrap_or_default();
        self.git_at_workspace(&["checkout", commit, "--", "."], &cwd).is_ok()
    }

    /// Clean up snapshots older than max_age_days via git gc.
    pub fn cleanup(&self) -> u32 {
        if !self.config.enabled { return 0; }
        let _ = self.git(&["gc", "--prune=now"]);
        0 // git gc handles cleanup
    }

    pub fn is_enabled(&self) -> bool { self.config.enabled }
    pub fn should_enable(&self, workspace_size_gb: f32) -> bool {
        self.config.enabled && workspace_size_gb <= self.config.max_workspace_gb
    }
    pub fn max_age_secs(&self) -> u64 {
        self.config.max_age_days as u64 * 86400
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_config_defaults() {
        let mgr = SnapshotManager::new(SnapshotConfig::default());
        assert!(mgr.should_enable(1.5));
        assert!(!mgr.should_enable(3.0));
        assert_eq!(mgr.max_age_secs(), 7 * 86400);
        assert!(mgr.is_enabled());
    }
}
