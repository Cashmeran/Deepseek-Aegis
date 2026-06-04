//! AgentTool — spawn sub-agents. CC AgentTool pattern.
//!
//! Uses a callback pattern: the actual sub-agent execution is injected
//! from spawn_agent where the concrete LLM client type is known.

use aegis_core::agent::subagent::{self, AgentDefinition, SubagentResult};
use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Async callback for running a sub-agent. Returns a boxed future.
pub type SubagentRunner = Arc<dyn Fn(AgentDefinition, String) -> Pin<Box<dyn Future<Output = SubagentResult> + Send>> + Send + Sync>;

pub struct AgentTool {
    builtins: Vec<AgentDefinition>,
    customs: Vec<AgentDefinition>,
    /// Callback that actually runs the sub-agent (injected from CLI).
    runner: Option<SubagentRunner>,
}

impl AgentTool {
    pub fn new() -> Self {
        Self {
            builtins: subagent::builtin_agents(),
            customs: Vec::new(),
            runner: None,
        }
    }

    pub fn with_customs(mut self, customs: Vec<AgentDefinition>) -> Self {
        self.customs = customs; self
    }

    pub fn with_runner(mut self, runner: SubagentRunner) -> Self {
        self.runner = Some(runner); self
    }

    fn find_agent(&self, name: &str) -> Option<&AgentDefinition> {
        subagent::find_agent(name, &self.builtins, &self.customs)
    }

    fn format_agent_list(&self) -> String {
        let mut out = String::new();
        for a in &self.builtins {
            out.push_str(&format!("- {}: {}\n", a.name, a.description));
        }
        for a in &self.customs {
            out.push_str(&format!("- {} (custom): {}\n", a.name, a.description));
        }
        out
    }

    pub fn agent_list_text(&self) -> String {
        self.format_agent_list()
    }
}

impl Default for AgentTool {
    fn default() -> Self { Self::new() }
}

impl ToolMetadata for AgentTool {
    fn schema(&self) -> ToolSchema {
        let agent_list = self.format_agent_list();
        ToolSchema {
            name: "agent".into(),
            description: "Launch a new agent to handle complex, multi-step tasks".into(),
            prompt: format!(
                "Launch a new agent to handle complex, multi-step tasks autonomously.\n\n\
                Available agent types:\n{}\n\
                Usage notes:\n\
                - Specify a subagent_type to select which agent type. Omit for general-purpose.\n\
                - Include a short description (3-5 words) of the task\n\
                - Write a clear prompt: explain what to do, what context matters, what not to do\n\
                - Launch multiple agents in one message for parallel work\n\
                - The agent's result is not visible to the user — summarize it for them\n\n\
                When NOT to use:\n\
                - Reading a specific file → use file_read\n\
                - Searching for a class/function → use grep or glob\n\
                - Trivial single-step tasks",
                agent_list,
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_type": {
                        "type": "string",
                        "description": "Agent type: Explore, Plan, GeneralPurpose, or custom agent name"
                    },
                    "description": {
                        "type": "string",
                        "description": "A short (3-5 word) description of the task"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The task for the sub-agent to perform"
                    }
                },
                "required": ["description", "prompt"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for AgentTool {
    async fn execute(
        self: Arc<Self>,
        tool_use: &ToolUse,
        _ctx: &ToolContext,
    ) -> AgentResult<ToolResultMessage> {
        let subagent_type = tool_use.input.get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("GeneralPurpose");
        let _description = tool_use.input.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("task");
        let prompt = tool_use.input.get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if prompt.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: "prompt is required".into() }],
                elapsed_ms: 0,
            });
        }

        let agent_def = match self.find_agent(subagent_type) {
            Some(a) => a.clone(),
            None => {
                let available: Vec<&str> = self.builtins.iter().chain(&self.customs)
                    .map(|a| a.name.as_str()).collect();
                return Ok(ToolResultMessage {
                    tool_use_id: tool_use.id.clone(), is_error: true,
                    content: vec![ContentBlock::Text {
                        text: format!("Unknown agent type '{}'. Available: {}",
                            subagent_type, available.join(", ")),
                    }],
                    elapsed_ms: 0,
                });
            }
        };

        // Run sub-agent via async callback
        let result = if let Some(ref runner) = self.runner {
            runner(agent_def.clone(), prompt.to_string()).await
        } else {
            SubagentResult {
                agent_name: agent_def.name.clone(),
                output: format!(
                    "Sub-agent runner not configured. Would run '{}' with prompt: {:.200}...",
                    agent_def.name, prompt),
                tokens_used: 0, elapsed_ms: 0, error: None,
                model: agent_def.model.unwrap_or_else(|| "inherit".into()),
            }
        };

        let header = format!(
            "[Sub-agent: {} | {} | {}ms | {} tokens]\n\n",
            result.agent_name, result.model, result.elapsed_ms, result.tokens_used,
        );

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(),
            is_error: result.error.is_some(),
            content: vec![ContentBlock::Text { text: format!("{}{}", header, result.output) }],
            elapsed_ms: result.elapsed_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_tool_has_builtins() {
        let tool = AgentTool::new();
        assert!(tool.find_agent("Explore").is_some());
        assert!(tool.find_agent("GeneralPurpose").is_some());
    }

    #[test]
    fn test_agent_tool_schema() {
        let tool = AgentTool::new();
        let schema = tool.schema();
        assert_eq!(schema.name, "agent");
        assert!(schema.prompt.contains("Explore"));
    }

    #[test]
    fn test_agent_tool_rejects_unknown() {
        let tool = AgentTool::new();
        assert!(tool.find_agent("nonexistent").is_none());
    }
}
