//! web_search — DeepSeek-native server-side web search.
//! The search execution and result summarization are handled entirely by DeepSeek's API.
//! This tool definition tells the model web search is available; results arrive as
//! `web_search_tool_result` blocks in the API response stream.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self { Self }
}

impl Default for WebSearchTool {
    fn default() -> Self { Self }
}

impl ToolMetadata for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".into(),
            description: "Search the web for current information, documentation, and facts".into(),
            prompt: "Use web_search to find current information on the internet.\n\
                     - Search is executed server-side by DeepSeek — no extra tool execution cost\n\
                     - Results include page titles, URLs, and snippets\n\
                     - Use for: current events, recent documentation, facts beyond training cutoff\n\
                     - Each search may trigger additional API calls for result summarization\n\
                     - Prefer specific queries over broad ones for better results".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query string"
                    },
                    "allowed_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Only include results from these domains"
                    },
                    "blocked_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Exclude results from these domains"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for WebSearchTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let query = tool_use.input.get("query").and_then(|v| v.as_str()).unwrap_or("");

        if query.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(),
                is_error: false,
                content: vec![ContentBlock::Text {
                    text: "Web search result will be returned by the API server.".into(),
                }],
                elapsed_ms: 0,
            });
        }

        // Server-side: DeepSeek handles the actual search. The result arrives via
        // web_search_tool_result blocks in the DeepSeek API response stream.
        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text {
                text: format!("Searching for: {}", query),
            }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_web_search_accepts_query() {
        let tool = Arc::new(WebSearchTool::new());
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000,
            ask_user_cb: Default::default(), progress_tx: None };
        let tu = ToolUse {
            id: "t1".into(),
            name: "web_search".into(),
            input: serde_json::json!({"query": "rust async patterns"}),
        };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(!r.is_error);
    }
}
