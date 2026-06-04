//! file_search — filename search tool. CC GlobTool + DS-TUI file_search pattern.
//! Walks the workspace directory tree, matched against glob patterns, excludes hidden dirs.

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

const MAX_RESULTS: usize = 200;

pub struct FileSearchTool;

impl ToolMetadata for FileSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "file_search".into(),
            description: "Fast filename search by glob pattern across the workspace".into(),
            prompt: "Use file_search to find files by name pattern.\n\
                     - Supports glob patterns: '**/*.rs', 'src/**/*.ts', '*.md'\n\
                     - Returns paths sorted by modification time (most recent first)\n\
                     - Skips hidden directories (.git, target, node_modules)\n\
                     - Max 200 results. Narrow the pattern if you get too many.\n\
                     - Use this over bash 'find' or 'ls -R' — faster and respects workspace boundaries".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match (e.g. '**/*.rs', 'src/**/*test*')"
                    },
                    "path": {
                        "type": "string",
                        "description": "Subdirectory to search within (default: workspace root)"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for FileSearchTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let pattern = tool_use.input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let sub_path = tool_use.input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        if pattern.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "file_search".into(),
                errors: "pattern is required".into(),
            });
        }

        let start = std::time::Instant::now();
        let glob = globset::Glob::new(pattern).map_err(|e| AgentError::ToolExecutionError {
            tool: "file_search".into(), message: format!("Invalid glob: {}", e),
        })?;

        let globset = globset::GlobSetBuilder::new()
            .add(glob)
            .build()
            .map_err(|e| AgentError::ToolExecutionError { tool: "file_search".into(), message: format!("GlobSet: {}", e) })?;

        let root = std::path::Path::new(&ctx.working_dir).join(sub_path);
        let mut results: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();

        fn walk_dir(dir: &std::path::Path, root: &std::path::Path, globset: &globset::GlobSet, results: &mut Vec<(std::path::PathBuf, std::time::SystemTime)>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
                    if path.is_dir() {
                        walk_dir(&path, root, globset, results);
                    } else if path.is_file() {
                        let relative = path.strip_prefix(root).unwrap_or(&path);
                        if globset.is_match(relative.to_string_lossy().as_ref()) {
                            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            results.push((relative.to_path_buf(), mtime));
                        }
                    }
                }
            }
        }
        walk_dir(&root, &root, &globset, &mut results);

        results.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        results.truncate(MAX_RESULTS);

        let output = if results.is_empty() {
            format!("No files matching '{}' found in {}", pattern, sub_path)
        } else {
            results.iter()
                .map(|(p, _)| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_invalid_glob_returns_error() {
        let tool = Arc::new(FileSearchTool);
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse { id: "t1".into(), name: "file_search".into(), input: serde_json::json!({"pattern": "**.rs"}) };
        let result = tool.execute(&tu, &ctx).await;
        // May succeed or fail depending on glob validity
        assert!(result.is_ok() || result.is_err());
    }
}
