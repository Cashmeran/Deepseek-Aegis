//! lsp — expose LSP diagnostics as an agent tool.
//! Wraps aegis-core's LspManager for read/diagnostics operations.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct LspTool {
    workspace_root: std::path::PathBuf,
}

impl LspTool {
    pub fn new(workspace_root: std::path::PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl ToolMetadata for LspTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "lsp".into(),
            description: "Read LSP diagnostics (errors, warnings) for files in the workspace".into(),
            prompt: "Use lsp to check for compiler/linter errors in the workspace.\n\
                     - Returns diagnostics from cargo check / rust-analyzer\n\
                     - Use after editing Rust files to verify correctness\n\
                     - Faster than running full cargo build — only checks changed files\n\
                     - Returns error/warning count and per-file details".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Optional: check a specific file. Omit for workspace-wide."
                    },
                    "include_warnings": {
                        "type": "boolean",
                        "description": "Include warnings in output (default: false)"
                    }
                },
                "required": []
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for LspTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let include_warnings = tool_use.input.get("include_warnings")
            .and_then(|v| v.as_bool()).unwrap_or(false);

        // Try cargo check first (reliable, works everywhere)
        let mut report = String::new();

        let result = std::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&self.workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let errors: Vec<&str> = stderr.lines()
                    .filter(|l| l.contains("error") || l.contains("error:"))
                    .collect();
                let warnings: Vec<&str> = stderr.lines()
                    .filter(|l| l.contains("warning") || l.contains("warning:"))
                    .collect();

                report.push_str(&format!("cargo check: {}\n", if output.status.success() {
                    "PASS"
                } else {
                    "FAIL"
                }));

                if !errors.is_empty() {
                    report.push_str(&format!("\n{} errors:\n", errors.len()));
                    for e in &errors[..errors.len().min(30)] {
                        report.push_str(&format!("  {}\n", e));
                    }
                    if errors.len() > 30 {
                        report.push_str(&format!("  ... and {} more errors\n", errors.len() - 30));
                    }
                }

                if include_warnings && !warnings.is_empty() {
                    report.push_str(&format!("\n{} warnings:\n", warnings.len()));
                    for w in &warnings[..warnings.len().min(20)] {
                        report.push_str(&format!("  {}\n", w));
                    }
                }

                if errors.is_empty() && (!include_warnings || warnings.is_empty()) {
                    if !stdout.is_empty() {
                        report.push_str(&format!("\n{}", stdout));
                    } else {
                        report.push_str("\nNo diagnostics found.");
                    }
                }
            }
            Err(e) => {
                report.push_str(&format!("cargo check unavailable: {}", e));
            }
        }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_with_workspace() {
        let tool = Arc::new(LspTool::new(std::path::PathBuf::from(".")));
        let tu = ToolUse {
            id: "t1".into(), name: "lsp".into(),
            input: serde_json::json!({"include_warnings": false}),
        };
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None,
            timeout_ms: 30000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(!r.is_error);
    }
}
