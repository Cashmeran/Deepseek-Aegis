//! todo_write — task tracking tool. CC TodoWriteTool + TaskCreateTool pattern.
//! In-memory task list for the current session.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
struct TaskEntry {
    subject: String,
    description: String,
    status: String, // pending, in_progress, completed
}

/// In-memory task store (per registry, session-scoped).
pub struct TodoWriteTool {
    tasks: Mutex<HashMap<String, Vec<TaskEntry>>>,
}

impl TodoWriteTool {
    pub fn new() -> Self {
        Self { tasks: Mutex::new(HashMap::new()) }
    }
}

impl Default for TodoWriteTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for TodoWriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo_write".into(),
            description: "Create and manage a structured task list for your current coding session".into(),
            prompt: "Use todo_write to create and track tasks.\n\n\
                     When to use:\n\
                     - Complex multi-step tasks (3+ distinct steps)\n\
                     - Non-trivial tasks that need planning\n\
                     - User provides multiple tasks\n\
                     - After receiving new instructions — capture requirements as tasks\n\
                     When NOT to use:\n\
                     - Single straightforward task — just do it\n\
                     - Trivial tasks where tracking adds overhead\n\n\
                     Task fields:\n\
                     - subject: Brief actionable title ('Fix auth bug')\n\
                     - description: What needs to be done\n\
                     - status: pending | in_progress | completed\n\
                     - Mark complete IMMEDIATELY after finishing — don't batch".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "Task objects to upsert",
                        "items": {
                            "type": "object",
                            "properties": {
                                "subject": {"type": "string", "description": "Task title"},
                                "description": {"type": "string", "description": "What to do"},
                                "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                            },
                            "required": ["subject", "status"]
                        }
                    }
                },
                "required": ["tasks"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TodoWriteTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let tasks_arr = tool_use.input.get("tasks").and_then(|v| v.as_array());
        let mut store = self.tasks.lock().unwrap();
        let session = store.entry(ctx.session_id.clone()).or_default();

        let mut report = String::from("## Task List\n\n");
        match tasks_arr {
            Some(items) => {
                for item in items {
                    let subject = item.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = item.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("pending");

                    // Upsert: find existing or add new
                    if let Some(existing) = session.iter_mut().find(|t| t.subject == subject) {
                        existing.status = status.to_string();
                        if !desc.is_empty() { existing.description = desc.to_string(); }
                    } else {
                        session.push(TaskEntry { subject: subject.to_string(), description: desc.to_string(), status: status.to_string() });
                    }
                }
            }
            None => {
                // No tasks array: just list current tasks
            }
        }

        for t in session.iter() {
            let icon = match t.status.as_str() {
                "completed" => "[x]", "in_progress" => "[>]", _ => "[ ]"
            };
            report.push_str(&format!("{} {} — {}\n", icon, t.subject, t.description));
        }

        if session.is_empty() { report.push_str("(no tasks)\n"); }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: report }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list_tasks() {
        let tool = Arc::new(TodoWriteTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test-session".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let input = serde_json::json!({"tasks": [
            {"subject": "Add login", "status": "in_progress"},
            {"subject": "Add tests", "status": "pending"}
        ]});
        let tu = ToolUse { id: "t1".into(), name: "todo_write".into(), input };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("Add login"));
        assert!(text.contains("Add tests"));
        assert!(text.contains("[>]")); // in_progress icon
    }
}
