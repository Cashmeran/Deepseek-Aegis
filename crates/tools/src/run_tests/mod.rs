//! run_tests — dedicated test runner. Generator's verification tool.
//! Wraps `cargo test` with output truncation and structured result parsing.

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

const MAX_OUTPUT_CHARS: usize = 40_000;

pub struct RunTestsTool;

impl ToolMetadata for RunTestsTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "run_tests".into(),
            description: "Run cargo test in the workspace with structured output".into(),
            prompt: "Use run_tests to run the test suite.\n\
                     - Runs `cargo test` with optional extra args and features\n\
                     - Output truncated at 40K chars\n\
                     - Returns: success/fail, exit code, stdout, stderr\n\
                     - The Generator runs this after making code changes\n\
                     - The Evaluator checks the result against acceptance criteria".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "args": {"type": "string", "description": "Extra cargo test args (e.g. '--lib')"},
                    "all_features": {"type": "boolean", "description": "Include --all-features flag"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentUnsafe }
}

#[async_trait]
impl Tool for RunTestsTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let extra_args = tool_use.input.get("args").and_then(|v| v.as_str()).unwrap_or("");
        let all_features = tool_use.input.get("all_features").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut cmd = std::process::Command::new("cargo");
        cmd.arg("test").current_dir(&ctx.working_dir);
        if all_features { cmd.arg("--all-features"); }
        if !extra_args.is_empty() {
            for arg in extra_args.split_whitespace() { cmd.arg(arg); }
        }
        cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

        let start = std::time::Instant::now();
        let output = cmd.output().map_err(|e| AgentError::ToolExecutionError {
            tool: "run_tests".into(), message: format!("cargo test: {}", e),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let success = output.status.success();

        // Parse test result summary from stderr (cargo test writes results there)
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut ignored = 0u32;
        let mut measured = 0u32;
        let mut filtered = 0u32;

        for line in stderr.lines() {
            if line.contains("test result:") {
                for part in line.split(';') {
                    let part = part.trim();
                    if let Some(num) = part.split_whitespace().next().and_then(|n| n.parse::<u32>().ok()) {
                        if part.contains("passed") { passed = num; }
                        else if part.contains("failed") { failed = num; }
                        else if part.contains("ignored") { ignored = num; }
                        else if part.contains("measured") { measured = num; }
                        else if part.contains("filtered") { filtered = num; }
                    }
                }
            }
        }

        let truncated_stdout = if stdout.len() > MAX_OUTPUT_CHARS {
            format!("{}...\n[truncated {} → {} chars]", &stdout[..MAX_OUTPUT_CHARS], stdout.len(), MAX_OUTPUT_CHARS)
        } else { stdout };

        let summary = if passed > 0 || failed > 0 {
            format!("{} passed, {} failed", passed, failed)
        } else {
            format!("exit={}", output.status.code().unwrap_or(-1))
        };

        let mut details = Vec::new();
        if ignored > 0 { details.push(format!("{} ignored", ignored)); }
        if measured > 0 { details.push(format!("{} measured", measured)); }
        if filtered > 0 { details.push(format!("{} filtered out", filtered)); }
        let detail_str = if details.is_empty() { String::new() } else { format!(" ({})", details.join(", ")) };

        let report = format!(
            "cargo test: {} — {}{}\n\nstdout:\n{}\n\nstderr:\n{}",
            if success { "PASS" } else { "FAIL" },
            summary, detail_str,
            truncated_stdout,
            stderr
        );

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_cargo_test_version() {
        let tool = Arc::new(RunTestsTool);
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 30000, ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse { id: "t1".into(), name: "run_tests".into(), input: serde_json::json!({"args": "--list"}) };
        let result = tool.execute(&tu, &ctx).await;
        assert!(result.is_ok());
    }
}
