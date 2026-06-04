//! sleep — wait/delay tool. CC SleepTool pattern.
//! The agent uses this when waiting for async external processes.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct SleepTool;

impl SleepTool {
    pub fn new() -> Self { Self }
}

impl Default for SleepTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for SleepTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "sleep".into(),
            description: "Pause execution for a specified duration (seconds)".into(),
            prompt: "Use sleep to wait for external processes or rate limits.\n\
                     - duration_secs: how long to sleep (1-300, max 5 minutes)\n\
                     - Use sparingly — prefer polling with tools over blind waiting".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "duration_secs": {"type": "integer", "description": "Seconds to sleep (1-300)"}
                },
                "required": ["duration_secs"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for SleepTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let secs = tool_use.input.get("duration_secs")
            .and_then(|v| v.as_u64()).unwrap_or(1).min(300);

        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Slept for {} second(s)", secs),
            }],
            elapsed_ms: secs * 1000,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sleep_short() {
        let tool = Arc::new(SleepTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse { id: "t1".into(), name: "sleep".into(),
            input: serde_json::json!({"duration_secs": 1}) };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(!r.is_error);
        assert!(r.elapsed_ms >= 900); // at least ~1 second
    }
}
