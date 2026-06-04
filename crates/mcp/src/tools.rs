//! MCP tools — MCPTool, ListMcpResourcesTool, ReadMcpResourceTool.
//! Registered as regular aegis tools, backed by McpConnectionManager.

use crate::manager::McpConnectionManager;
use aegis_core::{
    AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

// ═══════════════ MCPTool ═══════════════

/// Generic MCP tool executor. All MCP tools from connected servers are
/// accessible through this single tool. The agent passes the full tool name
/// (mcp__server__toolname) and arguments.
pub struct McpToolImpl {
    manager: Arc<McpConnectionManager>,
}

impl McpToolImpl {
    pub fn new(manager: Arc<McpConnectionManager>) -> Self { Self { manager } }
}

impl ToolMetadata for McpToolImpl {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp".into(),
            description: "Call MCP tools from connected MCP servers. Use list_mcp_resources to discover available servers and tools.".into(),
            prompt: "Use mcp to call tools from MCP servers.\n\
                     Call list_mcp_resources first to discover available servers.\n\
                     Tools are named mcp__server__toolname.\n\
                     Pass arguments as JSON in the 'arguments' field.\n\
                     MCP tools run on their own servers — follow the server's instructions.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string", "description": "MCP server name"},
                    "tool": {"type": "string", "description": "Tool name on the server"},
                    "arguments": {"type": "object", "description": "Tool arguments (JSON)"}
                },
                "required": ["server", "tool"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for McpToolImpl {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let server = tool_use.input.get("server").and_then(|v| v.as_str()).unwrap_or("");
        let tool = tool_use.input.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        let args = tool_use.input.get("arguments").cloned();

        if server.is_empty() || tool.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: "server and tool are required".into() }],
                elapsed_ms: 0,
            });
        }

        let full_name = format!("mcp__{}__{}", server, tool);
        match self.manager.call_tool(&full_name, args) {
            Ok(text) => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text }],
                elapsed_ms: 0,
            }),
            Err(e) => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: format!("MCP tool error: {}", e) }],
                elapsed_ms: 0,
            }),
        }
    }
}

// ═══════════════ ListMcpResourcesTool ═══════════════

pub struct ListMcpResourcesTool {
    manager: Arc<McpConnectionManager>,
}

impl ListMcpResourcesTool {
    pub fn new(manager: Arc<McpConnectionManager>) -> Self { Self { manager } }
}

impl ToolMetadata for ListMcpResourcesTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_mcp_resources".into(),
            description: "List connected MCP servers, their tools, and resources".into(),
            prompt: "Use list_mcp_resources to discover MCP servers and capabilities.\n\
                     Returns: connected servers, available MCP tools, and resource URIs.\n\
                     Use this before calling mcp tools to know what's available.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string", "description": "Filter to a specific server (optional)"}
                },
                "required": []
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ListMcpResourcesTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let filter = tool_use.input.get("server").and_then(|v| v.as_str());
        let states = self.manager.server_states();
        let tools = self.manager.all_tools();
        let resources = self.manager.all_resources();

        let mut out = String::from("## MCP Servers\n\n");
        if states.is_empty() {
            out.push_str("No MCP servers configured. Add to .mcp.json:\n");
            out.push_str(&crate::config::generate_default_mcp_json());
            out.push('\n');
        } else {
            for (name, state) in &states {
                let state_str = match state {
                    crate::types::ConnectionState::Connected => "+ connected",
                    crate::types::ConnectionState::Connecting => " connecting",
                    crate::types::ConnectionState::Disconnected => "x disconnected",
                    crate::types::ConnectionState::Failed(e) => &format!("x failed: {}", e),
                };
                if let Some(f) = filter {
                    if name != f { continue; }
                }
                out.push_str(&format!("**{}**: {}\n", name, state_str));
            }

            out.push_str("\n## MCP Tools\n\n");
            if tools.is_empty() {
                out.push_str("(no tools discovered)\n");
            } else {
                for t in &tools {
                    out.push_str(&format!("- `{}` — {}\n", t.name,
                        t.description.as_deref().unwrap_or("(no description)")));
                }
            }

            out.push_str("\n## MCP Resources\n\n");
            if resources.is_empty() {
                out.push_str("(no resources)\n");
            } else {
                for (server, r) in &resources {
                    out.push_str(&format!("- `{}` ({}): {}\n", r.uri, server,
                        r.description.as_deref().unwrap_or("(no description)")));
                }
            }
        }

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: false,
            content: vec![ContentBlock::Text { text: out }],
            elapsed_ms: 0,
        })
    }
}

// ═══════════════ ReadMcpResourceTool ═══════════════

pub struct ReadMcpResourceTool {
    manager: Arc<McpConnectionManager>,
}

impl ReadMcpResourceTool {
    pub fn new(manager: Arc<McpConnectionManager>) -> Self { Self { manager } }
}

impl ToolMetadata for ReadMcpResourceTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_mcp_resource".into(),
            description: "Read an MCP resource by URI from a connected MCP server".into(),
            prompt: "Use read_mcp_resource to read content from MCP resources.\n\
                     The URI must come from list_mcp_resources output.\n\
                     Resources can provide context, documentation, or data from MCP servers.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uri": {"type": "string", "description": "Resource URI (from list_mcp_resources)"}
                },
                "required": ["uri"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ReadMcpResourceTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let uri = tool_use.input.get("uri").and_then(|v| v.as_str()).unwrap_or("");

        if uri.is_empty() {
            return Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: "uri is required".into() }],
                elapsed_ms: 0,
            });
        }

        match self.manager.read_resource(uri) {
            Ok(text) => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: false,
                content: vec![ContentBlock::Text { text }],
                elapsed_ms: 0,
            }),
            Err(e) => Ok(ToolResultMessage {
                tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: format!("Read resource error: {}", e) }],
                elapsed_ms: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::McpConnectionManager;

    #[test]
    fn test_mcp_tools_without_manager() {
        let mgr = Arc::new(McpConnectionManager::new());
        let tool = McpToolImpl::new(mgr);
        assert_eq!(tool.schema().name, "mcp");
    }

    #[test]
    fn test_list_resources_empty() {
        let mgr = Arc::new(McpConnectionManager::new());
        let tool = ListMcpResourcesTool::new(mgr);
        assert!(tool.schema().name.contains("list_mcp_resources"));
    }

    #[test]
    fn test_read_resource_tool() {
        let mgr = Arc::new(McpConnectionManager::new());
        let tool = ReadMcpResourceTool::new(mgr);
        assert_eq!(tool.schema().name, "read_mcp_resource");
    }
}
