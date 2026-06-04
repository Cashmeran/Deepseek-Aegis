//! Workspace diagnostics probe — Planner's first tool in any session.
//! One call gives git status + toolchain versions + sandbox availability.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct DiagnosticsTool;

impl ToolMetadata for DiagnosticsTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "diagnostics".into(),
            description: "Probe workspace: git status, toolchain versions, sandbox availability".into(),
            prompt: "Use diagnostics at the start of a session to understand the workspace.\n\
                     Returns: git repo detection, current branch, rustc/cargo versions,\n\
                     sandbox status, OS info. No arguments needed.\n\
                     The Planner uses this first to orient before creating any plan.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for DiagnosticsTool {
    async fn execute(self: Arc<Self>, _tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let mut report = String::new();

        // OS
        report.push_str(&format!("OS: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));

        // Git
        let git_repo = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output().map(|o| o.status.success()).unwrap_or(false);
        if git_repo {
            let branch = std::process::Command::new("git")
                .args(["branch", "--show-current"])
                .output().ok().and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default().trim().to_string();
            let changed = std::process::Command::new("git")
                .args(["diff", "--stat"])
                .output().ok().and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();
            report.push_str(&format!("Git: branch={}", branch));
            if !changed.is_empty() {
                report.push_str(&format!(", uncommitted changes:\n{}", changed));
            } else { report.push('\n'); }
        } else { report.push_str("Git: not a repository\n"); }

        // Rust toolchain
        if let Ok(ver) = std::process::Command::new("rustc").arg("--version").output() {
            report.push_str(&format!("Rust: {}\n", String::from_utf8_lossy(&ver.stdout).trim()));
        }
        if let Ok(ver) = std::process::Command::new("cargo").arg("--version").output() {
            report.push_str(&format!("Cargo: {}\n", String::from_utf8_lossy(&ver.stdout).trim()));
        }

        // Sandbox
        report.push_str(&format!("Sandbox: {}\n", if ctx.sandbox_enabled { "enabled" } else { "disabled" }));

        Ok(ToolResultMessage {
            tool_use_id: _tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: 0,
        })
    }
}
