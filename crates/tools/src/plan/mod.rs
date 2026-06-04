//! plan tool — structured task planning integrated with SprintContract (三体架构).
//! CC EnterPlanModeTool + TaskCreate pattern, fused with our Planner→Generator→Evaluator.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct PlanTool;

impl ToolMetadata for PlanTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "plan".into(),
            description: "Create a structured implementation plan before writing code. Part of the three-body harness (Plan→Execute→Verify).".into(),
            prompt: "Use plan to create a structured implementation plan.\n\n\
                     In PLAN MODE: create the plan first, present it to user for approval.\n\
                     Use ask_user to share the plan and get approval before any edits.\n\
                     Only read/search tools are available — survey thoroughly.\n\n\
                     In AGENT MODE: create lightweight plans inline before coding.\n\
                     No need to ask for approval — execute immediately after planning.\n\n\
                     Plan structure:\n\
                     - objective: one-sentence goal\n\
                     - files: which files to create or modify\n\
                     - tasks: ordered concrete steps (each task = one todo)\n\
                     - acceptance: how to verify (commands + expected outputs)\n\
                     - constraints: things that MUST NOT be changed\n\n\
                     After creating plan: use todo_write to track each task.\n\
                     Mark complete as you finish. When all tasks done, run acceptance.\n\n\
                     Skip for trivial single-file edits — just do them.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "objective": {
                        "type": "string",
                        "description": "One-sentence goal of this plan"
                    },
                    "files": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Files to create or modify"
                    },
                    "tasks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "subject": {"type": "string"},
                                "description": {"type": "string"},
                                "depends_on": {"type": "array", "items": {"type": "string"}}
                            },
                            "required": ["subject"]
                        }
                    },
                    "acceptance": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "description": {"type": "string"},
                                "command": {"type": "string"},
                                "expected_exit": {"type": "integer"},
                                "expected_contains": {"type": "string"}
                            },
                            "required": ["description"]
                        }
                    },
                    "constraints": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Things that must NOT be changed"
                    }
                },
                "required": ["objective"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for PlanTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let objective = tool_use.input.get("objective").and_then(|v| v.as_str()).unwrap_or("(untitled)");
        let files = tool_use.input.get("files").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let tasks = tool_use.input.get("tasks").and_then(|v| v.as_array());
        let acceptance = tool_use.input.get("acceptance").and_then(|v| v.as_array());
        let constraints = tool_use.input.get("constraints").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut output = format!("## Plan: {}\n\n", objective);

        if !files.is_empty() {
            output.push_str("### Files\n");
            for f in &files { output.push_str(&format!("- {}\n", f)); }
            output.push('\n');
        }

        if let Some(tasks) = tasks {
            output.push_str("### Tasks\n");
            for (i, t) in tasks.iter().enumerate() {
                let subject = t.get("subject").and_then(|v| v.as_str()).unwrap_or("?");
                let desc = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let deps = t.get("depends_on").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let dep_str = if deps.is_empty() { String::new() } else { format!(" [depends: {}]", deps) };
                output.push_str(&format!("{}. {} — {}{}\n", i+1, subject, desc, dep_str));
            }
            output.push('\n');
        }

        if let Some(acceptance) = acceptance {
            output.push_str("### Acceptance Criteria\n");
            for a in acceptance {
                let desc = a.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let cmd = a.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let exit = a.get("expected_exit").and_then(|v| v.as_i64()).unwrap_or(0);
                output.push_str(&format!("- {}: `{}` (exit={})\n", desc, cmd, exit));
            }
            output.push('\n');
        }

        if !constraints.is_empty() {
            output.push_str("### Constraints (DO NOT CHANGE)\n");
            for c in &constraints { output.push_str(&format!("- {}\n", c)); }
            output.push('\n');
        }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plan_with_tasks_and_acceptance() {
        let tool = Arc::new(PlanTool);
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let input = serde_json::json!({
            "objective": "Add login page",
            "files": ["src/auth.rs", "src/login.rs"],
            "tasks": [
                {"subject": "Create auth module", "description": "JWT token generation"},
                {"subject": "Add login endpoint", "description": "POST /login", "depends_on": ["Create auth module"]}
            ],
            "acceptance": [
                {"description": "Login returns 200", "command": "curl -X POST /login", "expected_exit": 0}
            ],
            "constraints": ["Do not change database schema"]
        });
        let tu = ToolUse { id: "t1".into(), name: "plan".into(), input };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("login page"));
        assert!(text.contains("JWT"));
        assert!(text.contains("Do not change"));
    }
}
