//! list_dir — directory listing tool. CC list_files / DS-TUI directory_tree pattern.
//! The Planner uses this first to orient in unfamiliar workspaces.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ListDirTool;

impl ToolMetadata for ListDirTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_dir".into(),
            description: "List directory contents with file types and sizes".into(),
            prompt: "Use list_dir to understand directory structure.\n\
                     - Returns entries sorted: directories first, then files by name\n\
                     - Shows file sizes and types\n\
                     - Skips hidden entries (starting with .) by default\n\
                     - The Planner uses this as the first exploration tool\n\
                     - Use over bash 'ls' — respects workspace boundaries and is faster".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory to list (default: workspace root)"},
                    "depth": {"type": "integer", "description": "Recursion depth (1=flat, 2=one level down, max 3, default 1)"},
                    "show_hidden": {"type": "boolean", "description": "Include hidden files/dirs"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

fn format_size(bytes: u64) -> String {
    if bytes > 1_048_576 { format!("{:.1}M", bytes as f64 / 1_048_576.0) }
    else if bytes > 1024 { format!("{:.1}K", bytes as f64 / 1024.0) }
    else { format!("{}B", bytes) }
}

fn list(path: &std::path::Path, depth: usize, show_hidden: bool, prefix: &str) -> String {
    let mut out = String::new();
    let entries = match std::fs::read_dir(path) { Ok(e) => e, Err(_) => return out };

    let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    items.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| a.file_name().cmp(&b.file_name()))
    });

    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') { continue; }
        if name == "target" || name == "node_modules" { continue; }

        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let icon = if is_dir { "/" } else { "" };

        out.push_str(&format!("{}{}{}  {}\n", prefix, name, icon,
            if !is_dir { format!("({})", format_size(size)) } else { String::new() }));

        if is_dir && depth > 1 {
            let sub_prefix = format!("{}  ", prefix);
            out.push_str(&list(&entry.path(), depth - 1, show_hidden, &sub_prefix));
        }
    }
    out
}

#[async_trait]
impl Tool for ListDirTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let sub = tool_use.input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let depth = tool_use.input.get("depth").and_then(|v| v.as_u64()).unwrap_or(1).min(3) as usize;
        let show_hidden = tool_use.input.get("show_hidden").and_then(|v| v.as_bool()).unwrap_or(false);

        let full = ctx.working_dir.join(sub);
        let output = list(&full, depth, show_hidden, "");

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text {
                text: if output.is_empty() { "(empty directory)".into() } else { output }
            }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_dir_root() {
        let tool = Arc::new(ListDirTool);
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse { id: "t1".into(), name: "list_dir".into(), input: serde_json::json!({}) };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(r.content.iter().any(|b| matches!(b, ContentBlock::Text { .. })));
    }
}
