//! skill — invoke superpowers skills and plugins.
//! Follows CC SkillTool pattern: skill callbacks are injected during CLI setup.

use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct SkillTool {
    invoke: Option<Arc<dyn Fn(&str, &str) -> String + Send + Sync>>,
}

impl SkillTool {
    pub fn new() -> Self { Self { invoke: None } }

    pub fn with_backend(mut self, cb: Arc<dyn Fn(&str, &str) -> String + Send + Sync>) -> Self {
        self.invoke = Some(cb); self
    }
}

impl Default for SkillTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for SkillTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill".into(),
            description: "Execute a skill within the main conversation".into(),
            prompt: "When users ask you to perform tasks, check if any available skills match.\n\
                     Skills provide specialized capabilities and domain knowledge.\n\
                     - Set `skill` to the exact skill name (no leading slash)\n\
                     - For plugin-namespaced skills use `plugin:skill` format\n\
                     - Set `args` to pass optional arguments\n\
                     Only use this for registered skills. Do not guess skill names.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "The skill name (e.g. 'superpowers:brainstorming')"
                    },
                    "args": {
                        "type": "string",
                        "description": "Optional arguments for the skill"
                    }
                },
                "required": ["skill"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for SkillTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let skill = tool_use.input.get("skill").and_then(|v| v.as_str()).unwrap_or("");
        let args = tool_use.input.get("args").and_then(|v| v.as_str()).unwrap_or("");

        if skill.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(),
                is_error: true,
                content: vec![ContentBlock::Text {
                    text: "skill name is required. Use Skill tool with a valid skill name.".into(),
                }],
                elapsed_ms: 0,
            });
        }

        if let Some(ref invoke) = self.invoke {
            let result = invoke(skill, args);
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(),
                is_error: false,
                content: vec![ContentBlock::Text { text: result }],
                elapsed_ms: 0,
            })
        } else {
            Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(),
                is_error: false,
                content: vec![ContentBlock::Text {
                    text: format!("Skill '{}' registered but no backend connected. Install plugins to use skills.", skill),
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
    async fn test_skill_without_backend() {
        let tool = Arc::new(SkillTool::new());
        let tu = ToolUse {
            id: "t1".into(), name: "skill".into(),
            input: serde_json::json!({"skill": "superpowers:brainstorming"}),
        };
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        assert!(!r.is_error);
    }

    #[tokio::test]
    async fn test_skill_with_backend() {
        let tool = Arc::new(SkillTool::new().with_backend(Arc::new(|name, args| {
            format!("Loaded skill '{}' with args: {}", name, args)
        })));
        let tu = ToolUse {
            id: "t2".into(), name: "skill".into(),
            input: serde_json::json!({"skill": "superpowers:brainstorming", "args": "test"}),
        };
        let ctx = ToolContext {
            working_dir: ".".into(), permission_mode: aegis_core::types::PermissionMode::Default,
            session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None,
            timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await.unwrap();
        let text = r.content.iter().find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()), _ => None
        }).unwrap();
        assert!(text.contains("brainstorming"));
    }
}
