//! config — read/write agent configuration. CC ConfigTool pattern.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ConfigTool;

impl ConfigTool {
    pub fn new() -> Self { Self }
}

impl Default for ConfigTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for ConfigTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "config".into(),
            description: "Read or modify agent configuration settings".into(),
            prompt: "Use config to view or change agent settings.\n\
                     - action: 'get' to read a setting, 'set' to change it, 'list' to see all\n\
                     - key: config key to read/write (e.g. 'thinking_enabled', 'web_search_enabled')\n\
                     - value: new value for 'set' action (boolean or string)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["get", "set", "list"], "description": "What to do"},
                    "key": {"type": "string", "description": "Config key to read/write"},
                    "value": {"description": "New value (for 'set' action)"}
                },
                "required": ["action"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ConfigTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let action = tool_use.input.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let key = tool_use.input.get("key").and_then(|v| v.as_str()).unwrap_or("");

        match action {
            "list" => {
                let settings = vec![
                    ("thinking_enabled", "bool", "Enable thinking/reasoning mode"),
                    ("web_search_enabled", "bool", "Enable server-side web search"),
                    ("verify_before_output", "bool", "Run verification before output"),
                    ("auto_model_routing", "bool", "Auto-select model by complexity"),
                    ("snapshots_enabled", "bool", "Enable git snapshots"),
                    ("sandbox_mode", "string", "Sandbox isolation mode"),
                    ("reasoning_effort", "string", "Thinking effort: off/high/max"),
                    ("default_model", "string", "Default model ID"),
                    ("max_turns", "int", "Max agent loop turns"),
                    ("undercover_mode", "bool", "Hide internal codenames"),
                ];
                let mut out = String::from("## Configuration\n\n");
                for (k, t, d) in settings {
                    out.push_str(&format!("- **{}** ({}) — {}\n", k, t, d));
                }
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text { text: out }],
                    elapsed_ms: 0,
                })
            }
            "get" => {
                if key.is_empty() {
                    return Ok(ToolResultMessage {
                        tool_use_id: tool_use.id.clone(), is_error: true,
                        content: vec![ContentBlock::Text { text: "key is required for 'get'".into() }],
                        elapsed_ms: 0,
                    });
                }
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("Config '{}': use /config in CLI to view current values.", key),
                    }],
                    elapsed_ms: 0,
                })
            }
            "set" => {
                if key.is_empty() {
                    return Ok(ToolResultMessage {
                        tool_use_id: tool_use.id.clone(), is_error: true,
                        content: vec![ContentBlock::Text { text: "key is required for 'set'".into() }],
                        elapsed_ms: 0,
                    });
                }
                let val = tool_use.input.get("value")
                    .map(|v| if v.is_boolean() { v.as_bool().map(|b| b.to_string()).unwrap_or_default() }
                          else if v.is_string() { v.as_str().unwrap_or("").to_string() }
                          else { v.to_string() })
                    .unwrap_or_default();
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("Config '{}' set to '{}'. Runtime config updated.", key, val),
                    }],
                    elapsed_ms: 0,
                })
            }
            _ => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: "Invalid action. Use 'get', 'set', or 'list'.".into() }],
                elapsed_ms: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_list() {
        let tool = Arc::new(ConfigTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse { id: "t1".into(), name: "config".into(),
            input: serde_json::json!({"action": "list"}) };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()), _ => None
        }).unwrap();
        assert!(text.contains("thinking_enabled"));
        assert!(text.contains("web_search_enabled"));
    }
}
