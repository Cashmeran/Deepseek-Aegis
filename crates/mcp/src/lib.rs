// aegis-mcp: Full MCP (Model Context Protocol) implementation.
// Client-side: connect to MCP servers, discover tools/resources, execute.
// Server-side: expose aegis as an MCP server (health check, diagnostics).
//
// Architecture MCP infrastructure:
//   Transport → Client → Connection Manager → Tool Integration

pub mod types;
pub mod transport;
pub mod client;
pub mod manager;
pub mod config;
pub mod tools;
pub mod server;
pub mod health;
pub mod acp;

pub use types::*;
pub use transport::{McpTransport, SseTransport, StdioTransport};
pub use client::McpClient;
pub use manager::McpConnectionManager;
pub use config::{load_mcp_config, generate_default_mcp_json, McpConfig, McpServerEntry};
pub use tools::{McpToolImpl, ListMcpResourcesTool, ReadMcpResourceTool};
pub use server::McpServer;
pub use health::HealthStatus;
