use crate::types::tool::{EvaluatorMode, ExecutionMode};
use serde::{Deserialize, Serialize};

/// Agent 的全局配置。55 字段覆盖 LLM、上下文、记忆、沙箱、重试、安全等所有方面。
/// 默认值通过 `AgentConfig::default()` 提供，用户通过 `.agent/config.json` 覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    // ── 基本信息 ──
    pub name: String,
    pub version: String,
    pub default_model: String, // "deepseek-v4-flash"
    pub max_turns: u32,        // 100 — 防止死循环
    pub max_parallel_tools: usize, // 8 — 并行工具上限
    pub default_tool_timeout_ms: u64, // 600000 (10分钟, 对齐 CC)

    // ── 上下文多级折叠 (吸收 6级阈值) ──
    /// 最大上下文 token 数，0 = 自动从模型 × 0.8 计算
    pub max_context_tokens: usize,
    pub fold_threshold: f32,                // 0.75
    pub fold_aggressive_threshold: f32,     // 0.78
    pub force_summary_threshold: f32,       // 0.80
    pub turn_start_fold_threshold: f32,     // 0.90
    pub fold_min_savings_fraction: f32,     // 0.30
    pub fold_summary_timeout_ms: u64,       // 15000
    pub fold_tail_fraction: f32,            // 0.20
    pub fold_aggressive_tail_fraction: f32, // 0.10

    // ── 记忆与巩固 (吸收 DS-TUI + Claude Dream) ──
    pub memory_budget_tokens: usize,       // 1500
    pub consolidation_interval_hours: u32,  // 12
    pub consolidation_min_sessions: u32,    // 3
    pub user_memory_path: Option<String>,   // None = 不启用

    // ── 模型路由 ──
    pub auto_model_routing: bool,           // true
    pub router_light_max_complexity: f32,   // 0.3
    pub router_standard_max_complexity: f32,// 0.7

    // ── 验证与信心 ──
    pub verify_before_output: bool,         // true
    pub min_confidence_threshold: f32,      // 0.6

    // ── 执行模式 ──
    pub execution_mode: ExecutionMode,      // Agent (Shift+Tab 循环)
    pub evaluator_mode: EvaluatorMode,      // Auto

    // ── 沙箱 ──
    pub sandbox_mode: String,              // "workspace-write"
    pub sandbox_backend: String,           // "landlock"
    pub approval_policy: String,           // "on-request"

    // ── 重试 (Jitter) ──
    pub retry_max_attempts: u32,           // 4 (含首次)
    pub retry_initial_backoff_ms: u64,     // 500
    pub retry_max_backoff_ms: u64,         // 10000
    pub retry_jitter_enabled: bool,        // true (75%-125% jitter)
    pub retryable_statuses: Vec<u16>,      // [408,429,500,502,503,504]

    // ── 工具修复管线 (4-pass) ──
    pub repair_scavenge_enabled: bool,     // true
    pub repair_storm_window: usize,        // 6
    pub repair_storm_threshold: usize,     // 3
    pub repair_max_scavenge: usize,        // 4

    // ── 安全 ──
    pub undercover_mode: bool,             // true
    pub protected_files: Vec<String>,      // .gitconfig, .bashrc, ...

    // ── 快照 (吸收 DS-TUI side-git) ──
    pub snapshots_enabled: bool,           // true
    pub snapshots_max_age_days: u32,       // 7
    pub snapshots_max_workspace_gb: f32,   // 2.0

    // ── LSP ──
    pub lsp_poll_after_edit_ms: u64,       // 5000
    pub lsp_max_diagnostics_per_file: usize, // 20
    pub lsp_include_warnings: bool,        // false

    // ── 大输出路由 ──
    pub large_output_threshold_tokens: usize, // 4096

    // ── 通知 ──
    pub notifications_enabled: bool,       // true
    pub notification_threshold_secs: u64,  // 30

    // ── Provider 多租户 ──
    pub providers: Vec<ProviderConfig>,

    // ── 路径 ──
    pub code_graph_db_path: String,        // ".agent/code_graph.db"
    pub memory_db_path: String,            // ".agent/memory.db"
    pub workspace_dir: String,             // "."
    pub user_id: String,                   // "deepseek-aegis" (KVCache 隔离)

    // ── DeepSeek 特定 ──
    pub thinking_enabled: bool,            // true
    pub reasoning_effort: String,          // "max"
    pub use_beta_endpoint: bool,           // false — beta endpoint 未验证
    pub strict_tool_schema: bool,          // false — 需 beta endpoint
    pub web_search_enabled: bool,          // true (DeepSeek 服务端处理)
}

/// 单个 LLM Provider 的配置。
/// 支持 Anthropic、OpenAI、DeepSeek 等 12 种 Provider。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider 标识: "deepseek", "anthropic", "openai", ...
    pub name: String,
    /// API Base URL，None 使用 Provider 默认值
    pub api_base: Option<String>,
    /// 环境变量名，从中读取 API Key
    pub api_key_env: String,
    /// 默认模型 ID
    pub default_model: String,
    /// 是否启用此 Provider
    pub enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "Aegis".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            default_model: "deepseek-v4-pro".into(),
            max_turns: 100,
            max_parallel_tools: 8,
            default_tool_timeout_ms: 600_000,

            max_context_tokens: 0, // 0 = 自动从模型 × 0.8 计算
            fold_threshold: 0.75,
            fold_aggressive_threshold: 0.78,
            force_summary_threshold: 0.8,
            turn_start_fold_threshold: 0.9,
            fold_min_savings_fraction: 0.3,
            fold_summary_timeout_ms: 15_000,
            fold_tail_fraction: 0.2,
            fold_aggressive_tail_fraction: 0.1,

            memory_budget_tokens: 1500,
            consolidation_interval_hours: 12,
            consolidation_min_sessions: 3,
            user_memory_path: None,

            auto_model_routing: true,
            router_light_max_complexity: 0.3,
            router_standard_max_complexity: 0.7,

            verify_before_output: true,
            min_confidence_threshold: 0.6,
            execution_mode: ExecutionMode::Default,
            evaluator_mode: EvaluatorMode::Auto,

            sandbox_mode: "workspace-write".into(),
            sandbox_backend: "landlock".into(),
            approval_policy: "on-request".into(),

            retry_max_attempts: 4,
            retry_initial_backoff_ms: 500,
            retry_max_backoff_ms: 10_000,
            retry_jitter_enabled: true,
            retryable_statuses: vec![408, 429, 500, 502, 503, 504],

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
            snapshots_max_age_days: 7,
            snapshots_max_workspace_gb: 2.0,

            lsp_poll_after_edit_ms: 5_000,
            lsp_max_diagnostics_per_file: 20,
            lsp_include_warnings: false,

            large_output_threshold_tokens: 4_096,

            notifications_enabled: true,
            notification_threshold_secs: 30,

            providers: vec![],

            code_graph_db_path: ".agent/code_graph.db".into(),
            memory_db_path: ".agent/memory.db".into(),
            workspace_dir: ".".into(),
            user_id: "deepseek-aegis".into(),

            thinking_enabled: true,
            reasoning_effort: "max".into(),
            use_beta_endpoint: false,  // Beta endpoint 未验证，默认关闭
            strict_tool_schema: false,   // Strict schema 需要 beta endpoint
            web_search_enabled: true,  // 默认开启
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
        assert_eq!(cfg.retryable_statuses.len(), 6);
        assert_eq!(cfg.protected_files.len(), 6);
        assert!(cfg.undercover_mode);
        assert!(cfg.verify_before_output);
        assert!(cfg.providers.is_empty());
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

    #[test]
    fn test_provider_config_default_disabled() {
        let p = ProviderConfig {
            name: "test".into(),
            api_base: None,
            api_key_env: "TEST_KEY".into(),
            default_model: "test-model".into(),
            enabled: false,
        };
        assert!(!p.enabled);
        assert_eq!(p.api_base, None);
    }
}
