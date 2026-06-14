use crate::types::tool::ExecutionMode;
use serde::{Deserialize, Serialize};

/// Agent 的全局配置。精简后的活跃字段，去掉了未实现或被硬编码覆盖的占位项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    // ── 基本信息 ──
    pub name: String,
    pub default_model: String,
    pub max_turns: u32,
    pub max_parallel_tools: usize,
    pub default_tool_timeout_ms: u64,

    // ── 上下文多级折叠 ──
    pub max_context_tokens: usize,
    pub fold_threshold: f32,
    pub fold_aggressive_threshold: f32,
    pub force_summary_threshold: f32,
    pub turn_start_fold_threshold: f32,
    pub fold_min_savings_fraction: f32,
    pub fold_tail_fraction: f32,
    pub fold_aggressive_tail_fraction: f32,

    // ── 验证 ──
    pub verify_before_output: bool,

    // ── 沙箱 ──
    pub sandbox_mode: String,
    pub sandbox_backend: String,

    // ── 重试 ──
    pub retry_max_attempts: u32,
    pub retry_initial_backoff_ms: u64,

    // ── 工具修复管线 (4-pass) ──
    pub repair_scavenge_enabled: bool,
    pub repair_storm_window: usize,
    pub repair_storm_threshold: usize,
    pub repair_max_scavenge: usize,

    // ── 安全 ──
    pub undercover_mode: bool,
    pub protected_files: Vec<String>,

    // ── 快照 ──
    pub snapshots_enabled: bool,

    // ── LSP ──
    pub lsp_poll_after_edit_ms: u64,
    pub lsp_max_diagnostics_per_file: usize,
    pub lsp_include_warnings: bool,

    // ── 路径 ──
    pub workspace_dir: String,
    pub user_id: String,

    // ── DeepSeek 特定 ──
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
    pub strict_tool_schema: bool,
    pub web_search_enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "Aegis".into(),
            default_model: "deepseek-v4-pro".into(),
            max_turns: 100,
            max_parallel_tools: 8,
            default_tool_timeout_ms: 600_000,

            max_context_tokens: 0,
            fold_threshold: 0.75,
            fold_aggressive_threshold: 0.78,
            force_summary_threshold: 0.8,
            turn_start_fold_threshold: 0.9,
            fold_min_savings_fraction: 0.3,
            fold_tail_fraction: 0.2,
            fold_aggressive_tail_fraction: 0.1,

            verify_before_output: true,

            sandbox_mode: "workspace-write".into(),
            sandbox_backend: "landlock".into(),

            retry_max_attempts: 4,
            retry_initial_backoff_ms: 500,

            repair_scavenge_enabled: true,
            repair_storm_window: 6,
            repair_storm_threshold: 3,
            repair_max_scavenge: 4,

            undercover_mode: true,
            protected_files: vec![
                ".gitconfig".into(),
                ".bashrc".into(),
                ".zshrc".into(),
                ".mcp.json".into(),
                ".claude.json".into(),
                "~/.ssh/".into(),
            ],

            snapshots_enabled: true,

            lsp_poll_after_edit_ms: 5_000,
            lsp_max_diagnostics_per_file: 20,
            lsp_include_warnings: false,

            workspace_dir: ".".into(),
            user_id: "deepseek-aegis".into(),

            thinking_enabled: true,
            reasoning_effort: "max".into(),
            strict_tool_schema: false,
            web_search_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_sanity() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.name, "Aegis");
        assert_eq!(cfg.max_turns, 100);
        assert_eq!(cfg.max_parallel_tools, 8);
        assert_eq!(cfg.protected_files.len(), 6);
        assert!(cfg.undercover_mode);
        assert!(cfg.verify_before_output);
        assert_eq!(cfg.reasoning_effort, "max");
        assert!(cfg.web_search_enabled);
        assert!(!cfg.strict_tool_schema);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let cfg = AgentConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let decoded: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, cfg.name);
        assert_eq!(decoded.max_turns, cfg.max_turns);
        assert_eq!(decoded.fold_threshold, cfg.fold_threshold);
        assert!(decoded.undercover_mode);
    }
}
