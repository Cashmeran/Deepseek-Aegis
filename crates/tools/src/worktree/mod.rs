//! worktree tools — git worktree isolation. CC EnterWorktreeTool + ExitWorktreeTool pattern.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

fn run_git(args: &[&str], cwd: &str) -> Result<String, String> {
    std::process::Command::new("git")
        .args(args).current_dir(cwd)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .map_err(|e| format!("git: {}", e))
}

// ═══ enter_worktree ═══

pub struct EnterWorktreeTool;

impl EnterWorktreeTool {
    pub fn new() -> Self { Self }
}

impl Default for EnterWorktreeTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for EnterWorktreeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "enter_worktree".into(),
            description: "Create and enter an isolated git worktree for safe experimentation".into(),
            prompt: "Use enter_worktree to create an isolated workspace.\n\
                     - Creates a new git worktree in .claude/worktrees/\n\
                     - Pass name for the worktree (auto-generated if omitted)\n\
                     - Returns the worktree path — subsequent tools run in that directory".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Worktree name (auto-generated if omitted)"},
                    "path": {"type": "string", "description": "Existing worktree path to enter instead of creating"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentUnsafe }
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let name = tool_use.input.get("name").and_then(|v| v.as_str());
        let existing = tool_use.input.get("path").and_then(|v| v.as_str());

        let worktree_name = existing.map(String::from).unwrap_or_else(|| {
            name.map(String::from).unwrap_or_else(|| format!("aegis-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)))
        });

        let cwd = ctx.working_dir.to_string_lossy().to_string();

        if let Some(path) = existing {
            // Enter existing worktree
            match run_git(&["worktree", "list"], &cwd) {
                Ok(list) if list.contains(path) => {
                    Ok(ToolResultMessage {
                        tool_use_id: tool_use.id.clone(), is_error: false,
                        content: vec![ContentBlock::Text {
                            text: format!("Entered existing worktree: {}", path),
                        }],
                        elapsed_ms: 0,
                    })
                }
                _ => Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: true,
                    content: vec![ContentBlock::Text {
                        text: format!("Worktree '{}' not found in git worktree list", path),
                    }],
                    elapsed_ms: 0,
                }),
            }
        } else {
            // Create new worktree
            let wt_path = format!(".claude/worktrees/{}", worktree_name);
            match run_git(&["worktree", "add", &wt_path, "HEAD"], &cwd) {
                Ok(_) => {
                    Ok(ToolResultMessage {
                        tool_use_id: tool_use.id.clone(), is_error: false,
                        content: vec![ContentBlock::Text {
                            text: format!("Created worktree at {} on branch aegis-{}", wt_path, worktree_name),
                        }],
                        elapsed_ms: 0,
                    })
                }
                Err(e) => Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: true,
                    content: vec![ContentBlock::Text { text: format!("Failed: {}", e) }],
                    elapsed_ms: 0,
                }),
            }
        }
    }
}

// ═══ exit_worktree ═══

pub struct ExitWorktreeTool;

impl ExitWorktreeTool {
    pub fn new() -> Self { Self }
}

impl Default for ExitWorktreeTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for ExitWorktreeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "exit_worktree".into(),
            description: "Exit a worktree session and optionally remove it".into(),
            prompt: "Use exit_worktree to leave a worktree.\n\
                     - action: 'keep' (leave on disk) or 'remove' (delete worktree + branch)\n\
                     - discard_changes: must be true to remove uncommitted work".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["keep", "remove"], "description": "keep or remove the worktree"},
                    "discard_changes": {"type": "boolean", "description": "Force remove even with uncommitted changes"}
                },
                "required": ["action"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentUnsafe }
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let action = tool_use.input.get("action").and_then(|v| v.as_str()).unwrap_or("keep");
        let discard = tool_use.input.get("discard_changes").and_then(|v| v.as_bool()).unwrap_or(false);
        let cwd = ctx.working_dir.to_string_lossy().to_string();

        if action == "remove" && !discard {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text {
                    text: "Cannot remove worktree with uncommitted changes. Set discard_changes=true to force.".into(),
                }],
                elapsed_ms: 0,
            });
        }

        if action == "remove" {
            match run_git(&["worktree", "remove", &cwd, "--force"], &cwd) {
                Ok(_) => Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text { text: "Worktree removed.".into() }],
                    elapsed_ms: 0,
                }),
                Err(e) => Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: true,
                    content: vec![ContentBlock::Text { text: format!("Remove failed: {}", e) }],
                    elapsed_ms: 0,
                }),
            }
        } else {
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: "Worktree kept on disk.".into() }],
                elapsed_ms: 0,
            })
        }
    }
}
