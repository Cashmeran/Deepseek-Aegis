//! MCP JSON-RPC server — stdio transport for external editor integration.
//! Implements the Model Context Protocol (https://modelcontextprotocol.io/) subset.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};

/// MCP server — exposes aegis as an MCP endpoint for external editors.
pub struct McpServer;

impl McpServer {
    pub fn new() -> Self { Self }
}

impl Default for McpServer {
    fn default() -> Self { Self::new() }
}

/// MCP JSON-RPC request
#[derive(Debug, Deserialize)]
struct McpRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    #[allow(dead_code)]
    params: Value,
}

/// MCP JSON-RPC response
#[derive(Debug, Serialize)]
struct McpResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<McpError>,
}

#[derive(Debug, Serialize)]
struct McpError {
    code: i32,
    message: String,
}

/// Start MCP stdio server loop
pub fn run_stdio_loop() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    // Send initialization
    let init_response = McpResponse {
        jsonrpc: "2.0".into(),
        id: Some(Value::Number(0.into())),
        result: Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "aegis-mcp",
                "version": "0.1.0"
            },
            "capabilities": {
                "tools": {}
            }
        })),
        error: None,
    };
    writeln!(stdout, "{}", serde_json::to_string(&init_response)?)?;
    stdout.flush()?;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        let request: McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = McpResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(McpError { code: -32700, message: format!("Parse error: {}", e) }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let response = match request.method.as_str() {
            "initialize" => {
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: request.id,
                    result: Some(serde_json::json!({
                        "protocolVersion": "2024-11-05",
                        "serverInfo": { "name": "aegis-mcp", "version": "0.1.0" },
                        "capabilities": { "tools": {} }
                    })),
                    error: None,
                }
            }
            "tools/list" => {
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: request.id,
                    result: Some(serde_json::json!({
                        "tools": [
                            {"name": "aegis_run", "description": "Run an aegis agent task", "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "prompt": {"type": "string", "description": "The task to execute"}
                                },
                                "required": ["prompt"]
                            }},
                            {"name": "aegis_memory_query", "description": "Query the causal memory system", "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": {"type": "string", "description": "Search query for memory retrieval"},
                                    "limit": {"type": "integer", "description": "Max results (default 5)"}
                                },
                                "required": ["query"]
                            }}
                        ]
                    })),
                    error: None,
                }
            }
            "notifications/initialized" => {
                continue; // No response for notifications
            }
            _ => {
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: request.id,
                    result: None,
                    error: Some(McpError { code: -32601, message: format!("Method not found: {}", request.method) }),
                }
            }
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}
