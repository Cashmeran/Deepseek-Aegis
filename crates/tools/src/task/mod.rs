//! Task management tools — CC TaskCreate/Get/List/Update/Output/Stop pattern.
//! Share task store with todo_write via the same session-scoped HashMap.

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

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub subject: String,
    pub description: String,
    pub status: String,       // pending, in_progress, completed
    pub active_form: String,  // present continuous for spinner display
    pub metadata: serde_json::Value,
    pub depends_on: Vec<String>,
}

pub type TaskStore = Arc<Mutex<HashMap<String, Vec<TaskEntry>>>>;

fn task_status_icon(status: &str) -> &str {
    match status {
        "completed" => "[x]", "in_progress" => "[>]", "cancelled" => "[-]", _ => "[ ]",
    }
}

// ═══ task_create ═══

pub struct TaskCreateTool { tasks: TaskStore }

impl TaskCreateTool {
    pub fn new(tasks: TaskStore) -> Self { Self { tasks } }
}

impl ToolMetadata for TaskCreateTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_create".into(),
            description: "Create a new task in the task list".into(),
            prompt: "Use task_create to add new tasks to track progress.\n\
                     Fields: subject (brief title), description (details), depends_on (task subjects that block this one).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subject": {"type": "string", "description": "Brief actionable title"},
                    "description": {"type": "string", "description": "What needs to be done"},
                    "depends_on": {"type": "array", "items": {"type": "string"}, "description": "Task subjects this depends on"}
                },
                "required": ["subject"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskCreateTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let subject = tool_use.input.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let desc = tool_use.input.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let deps: Vec<String> = tool_use.input.get("depends_on").and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(String::from).collect())
            .unwrap_or_default();

        let mut store = self.tasks.lock().unwrap();
        let session = store.entry(ctx.session_id.clone()).or_default();

        if session.iter().any(|t| t.subject == subject) {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: format!("Task '{}' already exists", subject) }],
                elapsed_ms: 0,
            });
        }

        session.push(TaskEntry {
            subject: subject.to_string(), description: desc.to_string(),
            status: "pending".into(), active_form: String::new(),
            metadata: serde_json::json!({}), depends_on: deps,
        });

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: format!("Created task: {}", subject) }],
            elapsed_ms: 0,
        })
    }
}

// ═══ task_get ═══

pub struct TaskGetTool { tasks: TaskStore }

impl TaskGetTool {
    pub fn new(tasks: TaskStore) -> Self { Self { tasks } }
}

impl ToolMetadata for TaskGetTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_get".into(),
            description: "Get full details of a specific task".into(),
            prompt: "Use task_get to retrieve a specific task's full details including dependencies.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subject": {"type": "string", "description": "Task subject to retrieve"}
                },
                "required": ["subject"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskGetTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let subject = tool_use.input.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let store = self.tasks.lock().unwrap();
        let session = store.get(&ctx.session_id);

        match session.and_then(|s| s.iter().find(|t| t.subject == subject)) {
            Some(t) => {
                let deps = if t.depends_on.is_empty() { String::new() }
                    else { format!("\nDepends on: {}", t.depends_on.join(", ")) };
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("{} {} — {}{}", task_status_icon(&t.status), t.subject, t.description, deps),
                    }],
                    elapsed_ms: 0,
                })
            }
            None => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: format!("Task '{}' not found", subject) }],
                elapsed_ms: 0,
            }),
        }
    }
}

// ═══ task_list ═══

pub struct TaskListTool { tasks: TaskStore }

impl TaskListTool {
    pub fn new(tasks: TaskStore) -> Self { Self { tasks } }
}

impl ToolMetadata for TaskListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_list".into(),
            description: "List all tasks with status and progress summary".into(),
            prompt: "Use task_list to see all tasks and their statuses.\n\
                     Returns a summary of each task: id, subject, status, blocking dependencies.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskListTool {
    async fn execute(self: Arc<Self>, _tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let store = self.tasks.lock().unwrap();
        let session = store.get(&ctx.session_id);

        let mut output = String::from("## Tasks\n\n");
        if let Some(tasks) = session {
            if tasks.is_empty() {
                output.push_str("(no tasks)\n");
            } else {
                let done = tasks.iter().filter(|t| t.status == "completed").count();
                output.push_str(&format!("{} / {} complete\n\n", done, tasks.len()));
                for t in tasks {
                    let blocked = if !t.depends_on.is_empty() {
                        let unmet: Vec<&str> = t.depends_on.iter().filter(|d| {
                            !tasks.iter().any(|o| &o.subject == *d && o.status == "completed")
                        }).map(|s| s.as_str()).collect();
                        if unmet.is_empty() { String::new() }
                        else { format!(" [blocked by: {}]", unmet.join(", ")) }
                    } else { String::new() };
                    output.push_str(&format!("{} {} — {}{}\n",
                        task_status_icon(&t.status), t.subject, t.description, blocked));
                }
            }
        } else {
            output.push_str("(no tasks)\n");
        }

        Ok(ToolResultMessage {
            tool_use_id: _tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: 0,
        })
    }
}

// ═══ task_update ═══

pub struct TaskUpdateTool { tasks: TaskStore }

impl TaskUpdateTool {
    pub fn new(tasks: TaskStore) -> Self { Self { tasks } }
}

impl ToolMetadata for TaskUpdateTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_update".into(),
            description: "Update a task's status, description, or other fields".into(),
            prompt: "Use task_update to change task status or details.\n\
                     Status workflow: pending → in_progress → completed.\n\
                     Mark tasks in_progress BEFORE starting work.\n\
                     Mark tasks completed IMMEDIATELY after finishing.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subject": {"type": "string", "description": "Task subject to update"},
                    "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]},
                    "description": {"type": "string", "description": "New description (optional)"},
                    "active_form": {"type": "string", "description": "Present continuous form for spinner"}
                },
                "required": ["subject"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let subject = tool_use.input.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let status = tool_use.input.get("status").and_then(|v| v.as_str());
        let desc = tool_use.input.get("description").and_then(|v| v.as_str());
        let active = tool_use.input.get("active_form").and_then(|v| v.as_str());

        let mut store = self.tasks.lock().unwrap();
        let session = store.entry(ctx.session_id.clone()).or_default();

        match session.iter_mut().find(|t| t.subject == subject) {
            Some(t) => {
                if let Some(s) = status { t.status = s.to_string(); }
                if let Some(d) = desc { t.description = d.to_string(); }
                if let Some(a) = active { t.active_form = a.to_string(); }
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("{} {} → {}", task_status_icon(&t.status), t.subject, t.status),
                    }],
                    elapsed_ms: 0,
                })
            }
            None => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: format!("Task '{}' not found", subject) }],
                elapsed_ms: 0,
            }),
        }
    }
}

// ═══ task_output ═══

pub struct TaskOutputTool;

impl Default for TaskOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskOutputTool {
    pub fn new() -> Self { Self }
}

impl ToolMetadata for TaskOutputTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_output".into(),
            description: "Retrieve output from a running or completed background task".into(),
            prompt: "Use task_output to get results from background tasks (builds, tests, agents).\n\
                     - Takes a task_id parameter identifying the task\n\
                     - Use block=true to wait for completion\n\
                     - Use block=false for non-blocking status check".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task ID to get output from"},
                    "block": {"type": "boolean", "description": "Wait for completion (default: true)"},
                    "timeout": {"type": "integer", "description": "Max wait time in ms (default: 30000)"}
                },
                "required": ["task_id"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskOutputTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let task_id = tool_use.input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Task output for '{}': no background task system wired yet. Use task_list to check status.", task_id),
            }],
            elapsed_ms: 0,
        })
    }
}

// ═══ task_stop ═══

pub struct TaskStopTool;

impl Default for TaskStopTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStopTool {
    pub fn new() -> Self { Self }
}

impl ToolMetadata for TaskStopTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_stop".into(),
            description: "Stop a running background task by its ID".into(),
            prompt: "Use task_stop to terminate a long-running background task.\n\
                     - Takes a task_id parameter identifying the task to stop\n\
                     - Returns success or failure status".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task ID to stop"}
                },
                "required": ["task_id"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for TaskStopTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let task_id = tool_use.input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Task stop requested for '{}'. Background task system not wired yet.", task_id),
            }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> TaskStore {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test-session".into(), env: Default::default(), sandbox_enabled: false,
            sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None }
    }

    #[tokio::test]
    async fn test_create_and_list() {
        let store = test_store();
        let create = Arc::new(TaskCreateTool::new(store.clone()));
        let list = Arc::new(TaskListTool::new(store.clone()));

        let tu = ToolUse { id: "t1".into(), name: "task_create".into(),
            input: serde_json::json!({"subject": "Add login", "description": "JWT auth"}) };
        create.execute(&tu, &test_ctx()).await.unwrap();

        let tu2 = ToolUse { id: "t2".into(), name: "task_list".into(), input: serde_json::json!({}) };
        let r = list.execute(&tu2, &test_ctx()).await.unwrap();
        let text = r.content.iter().find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()), _ => None
        }).unwrap();
        assert!(text.contains("Add login"));
        assert!(text.contains("JWT"));
    }

    #[tokio::test]
    async fn test_update_mark_completed() {
        let store = test_store();
        let create = Arc::new(TaskCreateTool::new(store.clone()));
        let update = Arc::new(TaskUpdateTool::new(store.clone()));

        create.execute(&ToolUse { id: "t1".into(), name: "task_create".into(),
            input: serde_json::json!({"subject": "Fix bug"}) }, &test_ctx()).await.unwrap();

        let r = update.execute(&ToolUse { id: "t2".into(), name: "task_update".into(),
            input: serde_json::json!({"subject": "Fix bug", "status": "completed"}) }, &test_ctx()).await.unwrap();
        let text = r.content.iter().find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()), _ => None
        }).unwrap();
        assert!(text.contains("completed"));
    }
}
