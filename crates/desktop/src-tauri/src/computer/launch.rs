use async_trait::async_trait;
// LaunchApp tool — opens applications via cmd start.
use std::sync::Arc;
use aegis_core::error::AgentResult;
use aegis_core::types::tool::{ConcurrencySafety, RiskLevel, Tool, ToolMetadata, ToolSchema};
use aegis_core::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use aegis_core::types::tool::ToolContext;

pub struct LaunchAppTool;
impl LaunchAppTool { pub fn new() -> Self { Self } }
impl Default for LaunchAppTool { fn default() -> Self { Self } }

impl ToolMetadata for LaunchAppTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "launch_app".into(),
            description: "Launch a Windows application by name (e.g. 'notepad', 'chrome', 'calc') or full path to .exe.".into(),
            prompt: "Use launch_app to open applications. Provide the app name or full path.".into(),
            input_schema: serde_json::json!({"type":"object","properties":{"name":{"type":"string","description":"App name or .exe path"}},"required":["name"]}),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for LaunchAppTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let name = tu.input.get("name").and_then(|v| v.as_str()).unwrap_or("");
        std::process::Command::new("cmd").args(["/C","start","",name]).spawn()
            .map_err(|e| aegis_core::error::AgentError::Internal(format!("launch {name}: {e}")))?;
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Launched {name}") }], elapsed_ms: 0 })
    }
}
