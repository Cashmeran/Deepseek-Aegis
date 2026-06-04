//! tool_search — deferred tool lookup. CC ToolSearchTool pattern.
//! When tools are deferred (shouldDefer=true), the model calls this to discover them.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ToolSearchTool {
    tool_list: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

impl ToolSearchTool {
    pub fn new() -> Self { Self { tool_list: None } }

    pub fn with_tool_list(mut self, cb: Arc<dyn Fn() -> String + Send + Sync>) -> Self {
        self.tool_list = Some(cb); self
    }
}

impl Default for ToolSearchTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for ToolSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "tool_search".into(),
            description: "Search for available tools by keyword or description. Use when you need a tool not in the main list.".into(),
            prompt: "Use tool_search to find tools that aren't loaded by default.\n\
                     - Provide a query describing what you need ('search code', 'git worktree', 'schedule')\n\
                     - Returns matching tool names and descriptions\n\
                     - After finding a tool, you can call it normally by name".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What kind of tool are you looking for?"}
                },
                "required": ["query"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ToolSearchTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let query = tool_use.input.get("query").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(ref cb) = self.tool_list {
            let all = cb();
            let lower = query.to_lowercase();
            let matching: Vec<&str> = all.lines()
                .filter(|l| l.to_lowercase().contains(&lower))
                .take(20)
                .collect();

            if matching.is_empty() {
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("No tools found matching '{}'. Available tools:\n{}", query, all),
                    }],
                    elapsed_ms: 0,
                })
            } else {
                Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text {
                        text: format!("Tools matching '{}':\n{}", query, matching.join("\n")),
                    }],
                    elapsed_ms: 0,
                })
            }
        } else {
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text {
                    text: "Tool search not configured. Install aegis-tools with full registry.".into(),
                }],
                elapsed_ms: 0,
            })
        }
    }
}
