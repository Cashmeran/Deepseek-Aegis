//! MCP configuration loader — reads .mcp.json and settings.json mcpServers.

pub use crate::types::{McpConfig, McpServerEntry, McpTransportConfig};
use std::collections::HashMap;
use std::path::Path;

/// Load MCP server configuration from .mcp.json in the project root.
/// Falls back to .agent/config.json mcp_servers section.
pub fn load_mcp_config(base_dir: &Path) -> Result<McpConfig, String> {
    // 1. Try .mcp.json (CC standard)
    let mcp_json = base_dir.join(".mcp.json");
    if mcp_json.exists() {
        match std::fs::read_to_string(&mcp_json) {
            Ok(content) => {
                let config: McpConfig = serde_json::from_str(&content)
                    .map_err(|e| format!(".mcp.json parse error: {}", e))?;
                return Ok(config);
            }
            Err(e) => {
                tracing::warn!("Cannot read .mcp.json: {}", e);
            }
        }
    }

    // 2. Try .aegis/mcp.json
    let agent_config = base_dir.join(".aegis").join("mcp.json");
    if agent_config.exists() {
        match std::fs::read_to_string(&agent_config) {
            Ok(content) => {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content)
                    && let Some(mcp) = config.get("mcpServers") {
                        let servers: HashMap<String, McpServerEntry> = serde_json::from_value(mcp.clone())
                            .map_err(|e| format!("mcp.json parse error: {}", e))?;
                        return Ok(McpConfig { mcp_servers: servers });
                    }
            }
            Err(e) => {
                tracing::warn!("Cannot read .agent/config.json: {}", e);
            }
        }
    }

    // 3. Try user-level ~/.aegis/mcp.json
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    if let Some(home_dir) = home {
        let user_mcp = home_dir.join(".aegis").join("mcp.json");
        if user_mcp.exists() {
            match std::fs::read_to_string(&user_mcp) {
                Ok(content) => {
                    let config: McpConfig = serde_json::from_str(&content)
                        .map_err(|e| format!("~/.aegis/mcp.json parse error: {}", e))?;
                    return Ok(config);
                }
                Err(e) => {
                    tracing::warn!("Cannot read ~/.aegis/mcp.json: {}", e);
                }
            }
        }
    }

    // Default: empty config
    Ok(McpConfig::default())
}

/// Generate the MCP portion of .mcp.json (for /mcp init command).
pub fn generate_default_mcp_json() -> String {
    serde_json::to_string_pretty(&McpConfig {
        mcp_servers: HashMap::from([
            ("example".into(), McpServerEntry {
                name: None,
                transport: McpTransportConfig::Stdio(crate::types::StdioConfig {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-example".into()],
                    env: None,
                }),
                enabled: Some(false),
                description: Some("Example MCP server (disabled by default)".into()),
            }),
        ]),
    }).unwrap_or_default()
}
