//! ask_user — question tool. CC AskUserQuestionTool pattern.
//! Reads callback from ToolContext to show a TUI dialog.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct AskUserTool;

impl ToolMetadata for AskUserTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ask_user".into(),
            description: "Ask the user questions to gather information, clarify ambiguity, or get decisions".into(),
            prompt: "Use ask_user when you need user input during execution:\n\
                     - Gather preferences or requirements\n\
                     - Clarify ambiguous instructions\n\
                     - Get decisions on implementation choices\n\
                     - Ask before taking risky actions".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {"type": "string"},
                                "header": {"type": "string"},
                                "options": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {"type": "string"},
                                            "description": {"type": "string"}
                                        }
                                    }
                                },
                                "multiSelect": {"type": "boolean", "default": false}
                            }
                        }
                    }
                }
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for AskUserTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        eprintln!("[AskUserTool] callback present: {}", ctx.ask_user_cb.0.is_some());
        if let Some(ref cb) = ctx.ask_user_cb.0 {
            let input_json = serde_json::to_string(&tool_use.input).unwrap_or_default();
            let header = tool_use.input.get("questions")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|q| q.get("header").and_then(|v| v.as_str()))
                .unwrap_or("Question");
            let answer = cb(&input_json, header);
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text: answer }],
                elapsed_ms: 0,
            });
        }
        // Fallback: format question as text
        let mut output = String::from("## Questions for User\n\n");
        if let Some(qs) = tool_use.input.get("questions").and_then(|v| v.as_array()) {
            for (i, q) in qs.iter().enumerate() {
                let text = q.get("question").and_then(|v| v.as_str()).unwrap_or("");
                let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("");
                output.push_str(&format!("### Q{}: {} ({})\n", i+1, header, text));
                if let Some(opts) = q.get("options").and_then(|v| v.as_array()) {
                    for opt in opts {
                        let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("");
                        let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                        output.push_str(&format!("  - **{}**: {}\n", label, desc));
                    }
                }
            }
        }
        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: output }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ask_user_formats_questions() {
        let tool = Arc::new(AskUserTool);
        let input = serde_json::json!({"questions": [{
            "question": "Which library to use?",
            "header": "Library",
            "options": [
                {"label": "serde", "description": "Standard JSON"},
                {"label": "simd-json", "description": "Faster but less tested"}
            ],
            "multiSelect": false
        }]});
        let tu = ToolUse { id: "t1".into(), name: "ask_user".into(), input };
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b { ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap();
        assert!(text.contains("serde"));
        assert!(text.contains("simd-json"));
        assert!(text.contains("Q1"));
    }

    #[tokio::test]
    async fn test_callback_invoked_via_context() {
        let tool = Arc::new(AskUserTool);
        let input = serde_json::json!({"questions": [{
            "question": "Test?",
            "header": "Test",
            "options": [{"label": "A", "description": "Option A"}],
            "multiSelect": false
        }]});
        let tu = ToolUse { id: "t1".into(), name: "ask_user".into(), input };
        let cb: aegis_core::types::tool::AskUserCallback = Arc::new(|_q: &str, _h: &str| -> String {
            "user selected A".to_string()
        });
        let ctx = ToolContext {
            working_dir: ".".into(),
            permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "test".into(),
            env: Default::default(),
            sandbox_enabled: false,
            sandbox: None,
            timeout_ms: 5000,
            ask_user_cb: aegis_core::types::tool::DebugAskUserCb(Some(cb)),
            progress_tx: None,
        };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()), _ => None
        }).unwrap();
        assert_eq!(text, "user selected A", "Callback should return user's response");
    }
}
