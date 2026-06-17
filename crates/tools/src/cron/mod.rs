//! cron tools — schedule one-shot and recurring tasks. CC ScheduleCronTool pattern.

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
pub(crate) struct CronJob {
    id: String,
    cron: String,
    prompt: String,
    recurring: bool,
}

#[derive(Clone, Default)]
pub struct CronStore(Arc<Mutex<HashMap<String, Vec<CronJob>>>>);

impl CronStore {
    pub fn new() -> Self { Self(Arc::new(Mutex::new(HashMap::new()))) }
}

// ═══ cron_create ═══

pub struct CronCreateTool { jobs: CronStore }

impl CronCreateTool {
    pub fn new(jobs: CronStore) -> Self { Self { jobs } }
}

impl ToolMetadata for CronCreateTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "cron_create".into(),
            description: "Schedule a prompt to run at a future time (one-shot) or on a recurring schedule".into(),
            prompt: "Use cron_create to schedule tasks.\n\
                     - cron: 5-field expression (min hour dom month dow)\n\
                     - prompt: what to execute when triggered\n\
                     - recurring: true for repeat, false for one-shot\n\
                     Cron examples: '0 9 * * *' (9am daily), '*/5 * * * *' (every 5 min)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cron": {"type": "string", "description": "5-field cron expression in local time"},
                    "prompt": {"type": "string", "description": "The prompt to enqueue at fire time"},
                    "recurring": {"type": "boolean", "description": "Repeat on schedule (default: true)"}
                },
                "required": ["cron", "prompt"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for CronCreateTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let cron = tool_use.input.get("cron").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = tool_use.input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let recurring = tool_use.input.get("recurring").and_then(|v| v.as_bool()).unwrap_or(true);
        let id = format!("cron-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));

        if cron.is_empty() || prompt.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: "cron and prompt are required".into() }],
                elapsed_ms: 0,
            });
        }

        let job = CronJob { id: id.clone(), cron: cron.to_string(), prompt: prompt.to_string(), recurring };

        let mut store = self.jobs.0.lock().unwrap();
        store.entry(ctx.session_id.clone()).or_default().push(job);

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Cron job created: {} (id={}, recurring={})", cron, id, recurring),
            }],
            elapsed_ms: 0,
        })
    }
}

// ═══ cron_delete ═══

pub struct CronDeleteTool { jobs: CronStore }

impl CronDeleteTool {
    pub fn new(jobs: CronStore) -> Self { Self { jobs } }
}

impl ToolMetadata for CronDeleteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "cron_delete".into(),
            description: "Cancel a previously scheduled cron job".into(),
            prompt: "Use cron_delete to cancel a scheduled job by its ID.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Job ID returned by cron_create"}
                },
                "required": ["id"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for CronDeleteTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let id = tool_use.input.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let mut store = self.jobs.0.lock().unwrap();
        if let Some(jobs) = store.get_mut(&ctx.session_id)
            && let Some(pos) = jobs.iter().position(|j| j.id == id) {
                jobs.remove(pos);
                return Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text { text: format!("Job {} deleted", id) }],
                    elapsed_ms: 0,
                });
            }
        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: format!("Job {} not found", id) }],
            elapsed_ms: 0,
        })
    }
}

// ═══ cron_list ═══

pub struct CronListTool { jobs: CronStore }

impl CronListTool {
    pub fn new(jobs: CronStore) -> Self { Self { jobs } }
}

impl ToolMetadata for CronListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "cron_list".into(),
            description: "List all scheduled cron jobs".into(),
            prompt: "Use cron_list to see all scheduled jobs for this session.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for CronListTool {
    async fn execute(self: Arc<Self>, _tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let store = self.jobs.0.lock().unwrap();
        let mut output = String::from("## Cron Jobs\n\n");
        if let Some(jobs) = store.get(&ctx.session_id) {
            if jobs.is_empty() {
                output.push_str("(no cron jobs)\n");
            } else {
                for j in jobs {
                    output.push_str(&format!("- [{}] {} → {} (recurring={})\n",
                        &j.id[..8], j.cron, &j.prompt[..j.prompt.len().min(60)], j.recurring));
                }
            }
        } else {
            output.push_str("(no cron jobs)\n");
        }
        Ok(ToolResultMessage {
            tool_use_id: _tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: 0,
        })
    }
}
