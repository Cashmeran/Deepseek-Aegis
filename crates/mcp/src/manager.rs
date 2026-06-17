//! MCP Connection Manager — server lifecycle, reconnect, tool registry integration.

use crate::client::McpClient;
use crate::config::McpServerEntry;
use crate::transport::{McpTransport, SseTransport, StdioTransport};
use crate::types::{ConnectionState, McpResource, McpTool};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Manages multiple MCP server connections with lifecycle and discovery.
pub struct McpConnectionManager {
    clients: RwLock<HashMap<String, Arc<McpClient>>>,
    server_configs: RwLock<HashMap<String, McpServerEntry>>,
    state: RwLock<HashMap<String, ConnectionState>>,
    /// Discovered tools from all servers (by full tool name)
    all_tools: RwLock<Vec<McpTool>>,
    /// Discovered resources from all servers
    all_resources: RwLock<Vec<(String, McpResource)>>,
}

impl Default for McpConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpConnectionManager {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
            server_configs: RwLock::new(HashMap::new()),
            state: RwLock::new(HashMap::new()),
            all_tools: RwLock::new(Vec::new()),
            all_resources: RwLock::new(Vec::new()),
        }
    }

    /// Register server configs from .mcp.json.
    pub fn configure(&self, servers: HashMap<String, McpServerEntry>) {
        for (name, entry) in servers {
            if entry.enabled.unwrap_or(true) {
                self.server_configs.write().unwrap().insert(name.clone(), entry);
                self.state.write().unwrap().insert(name, ConnectionState::Disconnected);
            }
        }
    }

    /// Connect to a specific server.
    pub fn connect(&self, server_name: &str) -> Result<(), String> {
        let entry = self.server_configs.read().unwrap()
            .get(server_name).cloned()
            .ok_or_else(|| format!("Server '{}' not configured", server_name))?;

        self.state.write().unwrap().insert(server_name.into(), ConnectionState::Connecting);

        let transport: Arc<dyn McpTransport> = match &entry.transport {
            crate::types::McpTransportConfig::Stdio(cfg) => {
                Arc::new(StdioTransport::new(&cfg.command, &cfg.args, cfg.env.as_ref())?)
            }
            crate::types::McpTransportConfig::Sse(cfg) => {
                Arc::new(SseTransport::new(&cfg.url, cfg.headers.as_ref()))
            }
            crate::types::McpTransportConfig::Http(cfg) => {
                Arc::new(SseTransport::new(&cfg.url, cfg.headers.as_ref()))
            }
        };

        let mut client = McpClient::connect(server_name, transport)?;

        // Discover capabilities
        client.discover_tools().ok();
        client.discover_resources().ok();
        client.discover_prompts().ok();

        // Collect tools
        {
            let mut all = self.all_tools.write().unwrap();
            all.retain(|t| !t.name.starts_with(&format!("mcp__{}__", server_name)));
            all.extend(client.tools().to_vec());
        }

        // Collect resources
        {
            let mut all = self.all_resources.write().unwrap();
            all.retain(|(s, _)| s != server_name);
            for r in client.resources() {
                all.push((server_name.to_string(), r.clone()));
            }
        }

        let handle = Arc::new(client);
        self.clients.write().unwrap().insert(server_name.into(), handle);
        self.state.write().unwrap().insert(server_name.into(), ConnectionState::Connected);

        Ok(())
    }

    /// Connect to all configured servers.
    pub fn connect_all(&self) {
        let names: Vec<String> = self.server_configs.read().unwrap().keys().cloned().collect();
        for name in names {
            if let Err(e) = self.connect(&name) {
                self.state.write().unwrap().insert(name.clone(), ConnectionState::Failed(e.clone()));
                tracing::warn!("MCP server '{}' connection failed: {}", name, e);
            }
        }
    }

    /// Disconnect a server.
    pub fn disconnect(&self, server_name: &str) {
        if let Some(client) = self.clients.read().unwrap().get(server_name) {
            client.close();
        }
        self.clients.write().unwrap().remove(server_name);
        self.state.write().unwrap().insert(server_name.into(), ConnectionState::Disconnected);

        // Remove its tools
        self.all_tools.write().unwrap().retain(|t| !t.name.starts_with(&format!("mcp__{}__", server_name)));
        self.all_resources.write().unwrap().retain(|(s, _)| s != server_name);
    }

    /// Reconnect a failed server.
    pub fn reconnect(&self, server_name: &str) -> Result<(), String> {
        self.disconnect(server_name);
        self.connect(server_name)
    }

    /// Call a tool on the appropriate MCP server.
    pub fn call_tool(&self, full_name: &str, args: Option<serde_json::Value>) -> Result<String, String> {
        let server_name = extract_server_name(full_name)
            .ok_or_else(|| format!("Invalid MCP tool name: {}", full_name))?;

        let client = self.clients.read().unwrap()
            .get(server_name.as_str()).cloned()
            .ok_or_else(|| format!("MCP server '{}' not connected", server_name))?;

        let result = client.call_tool(full_name, args)?;

        let text: String = result.content.iter().filter_map(|c| match c {
            crate::types::ToolContent::Text { text } => Some(text.as_str()),
            crate::types::ToolContent::Resource { .. } => Some("[resource]"),
            crate::types::ToolContent::Image { .. } => Some("[image]"),
        }).collect::<Vec<_>>().join("\n");

        Ok(text)
    }

    /// Read a resource from its server.
    pub fn read_resource(&self, uri: &str) -> Result<String, String> {
        // Find which server has this resource
        let (server_name, _) = self.all_resources.read().unwrap().iter()
            .find(|(_, r)| r.uri == uri)
            .cloned()
            .ok_or_else(|| format!("Resource not found: {}", uri))?;

        let client = self.clients.read().unwrap()
            .get(&server_name).cloned()
            .ok_or_else(|| format!("MCP server '{}' not connected", server_name))?;

        let result = client.read_resource(uri)?;

        let text: String = result.contents.iter().filter_map(|c| match c {
            crate::types::ResourceContent::Text { text, .. } => Some(text.as_str()),
            crate::types::ResourceContent::Blob { blob, .. } => Some(blob.as_str()),
        }).collect::<Vec<_>>().join("\n");

        Ok(text)
    }

    // Accessors
    pub fn all_tools(&self) -> Vec<McpTool> { self.all_tools.read().unwrap().clone() }
    pub fn all_resources(&self) -> Vec<(String, McpResource)> { self.all_resources.read().unwrap().clone() }
    pub fn server_states(&self) -> HashMap<String, ConnectionState> { self.state.read().unwrap().clone() }
    pub fn client_count(&self) -> usize { self.clients.read().unwrap().len() }
}

/// Extract server name from mcp__server__toolname.
fn extract_server_name(full_name: &str) -> Option<String> {
    if !full_name.starts_with("mcp__") { return None; }
    let parts: Vec<&str> = full_name.splitn(4, "__").collect();
    if parts.len() >= 3 { Some(parts[2].to_string()) } else { None }
}
