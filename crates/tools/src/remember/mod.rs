//! remember — memory retrieval and storage tool. Exposes causal memory to the agent.
//! The agent uses this to recall past fixes and learn from corrections.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct RememberTool {
    /// Callback to memory crate — format: (action, key, value) → result text
    recall: Option<Arc<dyn Fn(&str, &str, &str) -> String + Send + Sync>>,
}

impl Default for RememberTool {
    fn default() -> Self {
        Self::new()
    }
}

impl RememberTool {
    pub fn new() -> Self { Self { recall: None } }

    /// Inject memory backend — called during CLI setup
    pub fn with_memory(mut self, cb: Arc<dyn Fn(&str, &str, &str) -> String + Send + Sync>) -> Self {
        self.recall = Some(cb); self
    }
}

impl ToolMetadata for RememberTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "remember".into(),
            description: "Query or store causal memory — past bugs, fixes, and insights".into(),
            prompt: "Use remember to access the causal memory system.\n\
                     Action 'query': search for past bugs/fixes/insights related to a query string.\n\
                     Action 'store': remember a new insight (e.g., 'null checks prevent NPE').\n\
                     The Planner uses this before coding to check for known pitfalls.\n\
                     The Evaluator uses this after fixing to record what was learned.\n\
                     Memory is cross-session — lessons persist across conversations.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["query", "store"], "description": "query past experiences or store new insight"},
                    "query": {"type": "string", "description": "Search query (for action=query)"},
                    "insight": {"type": "string", "description": "What was learned (for action=store)"}
                },
                "required": ["action"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for RememberTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let action = tool_use.input.get("action").and_then(|v| v.as_str()).unwrap_or("query");
        let query = tool_use.input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let insight = tool_use.input.get("insight").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(ref recall) = self.recall {
            let result = recall(action, query, insight);
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: result }],
                elapsed_ms: 0,
            })
        } else {
            // No memory backend — helpful fallback
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text {
                    text: "Memory system not configured. Install and wire aegis-memory crate.".into()
                }],
                elapsed_ms: 0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remember_query_without_backend() {
        let tool = Arc::new(RememberTool::new());
        let tu = ToolUse { id: "t1".into(), name: "remember".into(), input: serde_json::json!({"action": "query", "query": "null pointer"}) };
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(r.content.iter().any(|b| matches!(b, ContentBlock::Text { .. })));
    }

    #[tokio::test]
    async fn test_remember_with_backend() {
        let tool = Arc::new(RememberTool::new().with_memory(Arc::new(|action, query, _insight| {
            format!("{} result for: {}", action, query)
        })));
        let tu = ToolUse { id: "t2".into(), name: "remember".into(), input: serde_json::json!({"action": "query", "query": "null check"}) };
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("query result"));
    }
}
