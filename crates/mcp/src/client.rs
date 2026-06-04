//! MCP client — implements the full Model Context Protocol lifecycle.
//! - initialize → list tools/resources/prompts → call tool → read resource

use crate::transport::McpTransport;
use crate::types::*;
use std::sync::Arc;

pub struct McpClient {
    server_name: String,
    transport: Arc<dyn McpTransport>,
    server_info: ServerInfo,
    capabilities: ServerCapabilities,
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    prompts: Vec<McpPrompt>,
    initialized: bool,
}

impl McpClient {
    /// Connect to an MCP server and perform the initialize handshake.
    pub fn connect(
        server_name: &str,
        transport: Arc<dyn McpTransport>,
    ) -> Result<Self, String> {
        let req = JsonRpcRequest::new(
            0,
            "initialize",
            serde_json::to_value(InitializeParams {
                protocol_version: "2024-11-05".into(),
                capabilities: ClientCapabilities::default(),
                client_info: ClientInfo {
                    name: "aegis".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            }).map_err(|e| format!("Serialize init params: {}", e))?,
        );

        let resp = transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("Initialize failed: {} (code {})", err.message, err.code));
        }

        let init_result: InitializeResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty initialize result".to_string())?
        ).map_err(|e| format!("Parse init result: {}", e))?;

        // Send initialized notification
        transport.send_notification(&JsonRpcRequest::notification(
            "notifications/initialized",
            serde_json::json!({}),
        )).ok(); // Best-effort

        Ok(Self {
            server_name: server_name.into(),
            transport,
            server_info: init_result.server_info,
            capabilities: init_result.capabilities,
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
            initialized: true,
        })
    }

    /// Discover all tools from the server.
    pub fn discover_tools(&mut self) -> Result<&[McpTool], String> {
        if !self.capabilities.tools.is_some() {
            return Ok(&self.tools); // No tools capability
        }

        let req = JsonRpcRequest::new(1, "tools/list", serde_json::json!({}));
        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("tools/list failed: {}", err.message));
        }

        let result: ListToolsResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse tools/list: {}", e))?;

        // Prepend server name to tool names to avoid collisions
        self.tools = result.tools.into_iter().map(|mut t| {
            t.name = format!("mcp__{}__{}", self.server_name, t.name);
            t
        }).collect();

        Ok(&self.tools)
    }

    /// Discover all resources from the server.
    pub fn discover_resources(&mut self) -> Result<&[McpResource], String> {
        if !self.capabilities.resources.is_some() {
            return Ok(&self.resources);
        }

        let req = JsonRpcRequest::new(2, "resources/list", serde_json::json!({}));
        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("resources/list failed: {}", err.message));
        }

        let result: ListResourcesResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse resources/list: {}", e))?;

        self.resources = result.resources;
        Ok(&self.resources)
    }

    /// Discover all prompts from the server.
    pub fn discover_prompts(&mut self) -> Result<&[McpPrompt], String> {
        if !self.capabilities.prompts.is_some() {
            return Ok(&self.prompts);
        }

        let req = JsonRpcRequest::new(3, "prompts/list", serde_json::json!({}));
        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("prompts/list failed: {}", err.message));
        }

        let result: ListPromptsResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse prompts/list: {}", e))?;

        self.prompts = result.prompts;
        Ok(&self.prompts)
    }

    /// Call an MCP tool. Tool name should include the mcp__server__ prefix.
    pub fn call_tool(&self, full_name: &str, arguments: Option<serde_json::Value>) -> Result<CallToolResult, String> {
        // Strip mcp__server__ prefix to get the real tool name
        let real_name = if full_name.starts_with("mcp__") {
            let parts: Vec<&str> = full_name.splitn(4, "__").collect();
            if parts.len() >= 4 { parts[3] } else { parts[2] }
        } else {
            full_name
        };

        let params = CallToolParams {
            name: real_name.to_string(),
            arguments,
        };

        let req = JsonRpcRequest::new(
            100,
            "tools/call",
            serde_json::to_value(&params).map_err(|e| format!("Serialize: {}", e))?,
        );

        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("tools/call '{}' failed: {} (code {})", full_name, err.message, err.code));
        }

        let result: CallToolResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse tools/call: {}", e))?;

        Ok(result)
    }

    /// Read an MCP resource by URI.
    pub fn read_resource(&self, uri: &str) -> Result<ReadResourceResult, String> {
        let params = ReadResourceParams { uri: uri.to_string() };

        let req = JsonRpcRequest::new(
            101,
            "resources/read",
            serde_json::to_value(&params).map_err(|e| format!("Serialize: {}", e))?,
        );

        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("resources/read '{}' failed: {}", err.message, uri));
        }

        let result: ReadResourceResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse resources/read: {}", e))?;

        Ok(result)
    }

    /// Get a prompt by name.
    pub fn get_prompt(&self, name: &str, arguments: Option<serde_json::Value>) -> Result<GetPromptResult, String> {
        let req = JsonRpcRequest::new(
            102,
            "prompts/get",
            serde_json::json!({ "name": name, "arguments": arguments.unwrap_or(serde_json::json!({})) }),
        );

        let resp = self.transport.send_request(&req)?;

        if let Some(err) = resp.error {
            return Err(format!("prompts/get '{}' failed: {}", err.message, name));
        }

        let result: GetPromptResult = serde_json::from_value(
            resp.result.ok_or_else(|| "Empty result".to_string())?
        ).map_err(|e| format!("Parse prompts/get: {}", e))?;

        Ok(result)
    }

    // Accessors
    pub fn server_name(&self) -> &str { &self.server_name }
    pub fn server_info(&self) -> &ServerInfo { &self.server_info }
    pub fn tools(&self) -> &[McpTool] { &self.tools }
    pub fn resources(&self) -> &[McpResource] { &self.resources }
    pub fn prompts(&self) -> &[McpPrompt] { &self.prompts }
    pub fn is_connected(&self) -> bool { self.initialized && self.transport.is_alive() }

    /// Close the connection.
    pub fn close(&self) {
        self.transport.close();
    }
}
