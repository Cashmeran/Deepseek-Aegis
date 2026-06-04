//! Git tools — structured git diagnostics. Planner/Generator/Evaluator shared.
//! git_status, git_diff, git_log — safer and more reliable than bash git commands.

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

fn run_git(args: &[&str], cwd: &str) -> AgentResult<String> {
    let output = std::process::Command::new("git")
        .args(args).current_dir(cwd)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| AgentError::ToolExecutionError { tool: "git".into(), message: format!("{}", e) })?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn truncate(s: String) -> String {
    if s.len() > MAX_OUTPUT_CHARS {
        format!("{}...\n[truncated {} chars]", &s[..MAX_OUTPUT_CHARS], s.len())
    } else { s }
}

// ═══ git_status ═══

pub struct GitStatusTool;

impl ToolMetadata for GitStatusTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_status".into(),
            description: "Show working tree status (porcelain format)".into(),
            prompt: "Use git_status to check what files have changed.\n\
                     The Planner uses this to understand the current state before planning.\n\
                     The Generator uses this to review changes before committing.\n\
                     The Evaluator uses this to verify only expected files were modified.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Optional subdirectory to scope"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for GitStatusTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let path = tool_use.input.get("path").and_then(|v| v.as_str());
        let mut args = vec!["status", "--porcelain=v1", "-b"];
        if let Some(p) = path { args.push(p); }
        let output = truncate({ let cwd = ctx.working_dir.to_string_lossy(); run_git(&args, &cwd)? });
        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: output }], elapsed_ms: 0 })
    }
}

// ═══ git_diff ═══

pub struct GitDiffTool;

impl ToolMetadata for GitDiffTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_diff".into(),
            description: "Show changes between working tree and index/HEAD".into(),
            prompt: "Use git_diff to inspect changes in detail.\n\
                     The Generator uses this to review its own code before reporting complete.\n\
                     The Evaluator uses this to verify the diff matches the plan's expected_files.\n\
                     Options: staged (--cached), path (scope to file/dir), unified (context lines).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "staged": {"type": "boolean", "description": "Show staged changes only"},
                    "path": {"type": "string", "description": "Scope to file or directory"},
                    "unified": {"type": "integer", "description": "Context lines (default 3, max 50)"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for GitDiffTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let staged = tool_use.input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let path = tool_use.input.get("path").and_then(|v| v.as_str());
        let unified = tool_use.input.get("unified").and_then(|v| v.as_u64()).unwrap_or(3).min(50);

        let unified_flag = format!("-U{}", unified);
        let mut args = vec!["diff"];
        if staged { args.push("--cached"); }
        args.push(&unified_flag);
        if let Some(p) = path { args.push(p); }

        let output = truncate({ let cwd = ctx.working_dir.to_string_lossy(); run_git(&args, &cwd)? });
        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: output }], elapsed_ms: 0 })
    }
}

// ═══ git_log ═══

pub struct GitLogTool;

impl ToolMetadata for GitLogTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_log".into(),
            description: "Show commit history with optional file/path filters".into(),
            prompt: "Use git_log to understand the project's recent history.\n\
                     The Planner uses this to understand the codebase evolution.\n\
                     Options: max_count, path, author, since, oneline format.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "max_count": {"type": "integer", "description": "Max commits to show (default 20)"},
                    "path": {"type": "string", "description": "Filter to file or directory"},
                    "author": {"type": "string", "description": "Filter by author"},
                    "since": {"type": "string", "description": "e.g. '2024-01-01', '2 weeks ago'"},
                    "oneline": {"type": "boolean", "description": "Compact one-line format"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for GitLogTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let max = tool_use.input.get("max_count").and_then(|v| v.as_u64()).unwrap_or(20);
        let path = tool_use.input.get("path").and_then(|v| v.as_str());
        let author = tool_use.input.get("author").and_then(|v| v.as_str());
        let since = tool_use.input.get("since").and_then(|v| v.as_str());
        let oneline = tool_use.input.get("oneline").and_then(|v| v.as_bool()).unwrap_or(false);

        let max_flag = format!("-{}", max);
        let author_flag = author.map(|a| format!("--author={}", a));
        let since_flag = since.map(|s| format!("--since={}", s));
        let mut args = vec!["log", &max_flag];
        if oneline { args.push("--oneline"); }
        if let Some(ref a) = author_flag { args.push(a); }
        if let Some(ref s) = since_flag { args.push(s); }
        if let Some(p) = path { args.push("--"); args.push(p); }

        let output = truncate({ let cwd = ctx.working_dir.to_string_lossy(); run_git(&args, &cwd)? });
        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: output }], elapsed_ms: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_core::types::tool::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None }
    }

    #[tokio::test] async fn test_git_status() {
        let r = Arc::new(GitStatusTool).execute(&ToolUse { id: "t1".into(), name: "git_status".into(), input: serde_json::json!({}) }, &test_ctx()).await;
        assert!(r.is_ok());
    }

    #[tokio::test] async fn test_git_log_oneline() {
        let tu = ToolUse { id: "t2".into(), name: "git_log".into(), input: serde_json::json!({"max_count": 3, "oneline": true}) };
        let r = Arc::new(GitLogTool).execute(&tu, &test_ctx()).await;
        assert!(r.is_ok());
    }
}
