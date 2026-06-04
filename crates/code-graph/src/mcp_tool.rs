use crate::query::get_architectural_context;
use crate::store::GraphStore;
use aegis_core::error::{AgentError, AgentResult};
use aegis_core::types::{
    ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext, ToolMetadata, ToolResultMessage,
    ToolSchema, ToolUse,
};
use async_trait::async_trait;
use std::sync::Arc;

/// MCP 工具: 获取文件架构上下文。
/// CodeCompass 论文核心工程化实现。
pub struct ArchitecturalContextTool {
    store: Arc<dyn GraphStore>,
}

impl ArchitecturalContextTool {
    pub fn new(store: Arc<dyn GraphStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for ArchitecturalContextTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        _ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let file_path = tool_use
            .input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if file_path.is_empty() {
            return Err(AgentError::ToolValidationError {
                tool: "get_architectural_context".into(),
                errors: "file_path is required".into(),
            });
        }

        let start = std::time::Instant::now();
        let context_text = get_architectural_context(self.store.as_ref(), file_path)?;
        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: false,
            content: vec![ContentBlock::Text {
                text: context_text,
            }],
            elapsed_ms: elapsed,
        })
    }
}

impl ToolMetadata for ArchitecturalContextTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "get_architectural_context".into(),
            description: "Returns 1-hop architectural context for a file: imports, callers, callees, inheritance".into(),
            prompt: "Use BEFORE editing any file to understand its relationships.\nReturns: IMPORTS, IMPORTED_BY, CALLS, CALLED_BY, INHERITS.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file to query"
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::ConcurrentSafe
    }
}
