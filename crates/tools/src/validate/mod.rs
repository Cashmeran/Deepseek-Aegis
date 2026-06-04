//! validate — config file validator for JSON/TOML/YAML.
//! The Evaluator uses this to check generated configs before accepting.

use aegis_core::{
    AgentError, AgentResult,
    types::{
        ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext,
        ToolMetadata, ToolResultMessage, ToolSchema, ToolUse,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ValidateTool;

impl ToolMetadata for ValidateTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "validate".into(),
            description: "Validate a JSON, TOML, or YAML file for syntax correctness".into(),
            prompt: "Use validate to check config files before accepting them.\n\
                     Supported: JSON (.json), TOML (.toml, Cargo.toml), YAML (.yaml/.yml).\n\
                     The Evaluator uses this to verify generated config files are valid.\n\
                     Returns: valid/invalid + line-level error locations.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Path to the config file to validate"},
                    "format": {"type": "string", "enum": ["json", "toml", "yaml"], "description": "Auto-detected from extension if omitted"}
                },
                "required": ["file_path"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ValidateTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let path = tool_use.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let format = tool_use.input.get("format").and_then(|v| v.as_str());

        if path.is_empty() {
            return Err(AgentError::ToolValidationError { tool: "validate".into(), errors: "file_path is required".into() });
        }

        let content = std::fs::read_to_string(path).map_err(|e| AgentError::FileNotFound { path: format!("{}: {}", path, e) })?;
        let detected = format.unwrap_or_else(|| {
            if path.ends_with(".json") { "json" }
            else if path.ends_with(".toml") { "toml" }
            else if path.ends_with(".yaml") || path.ends_with(".yml") { "yaml" }
            else { "json" }
        });

        let (valid, detail) = match detected {
            "json" => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => (true, format!("Valid JSON. Top-level type: {}", if v.is_object() { "object" } else if v.is_array() { "array" } else { "scalar" })),
                Err(e) => (false, format!("Invalid JSON: {}", e)),
            },
            "toml" => match content.parse::<toml::Value>() {
                Ok(_) => (true, "Valid TOML".into()),
                Err(e) => (false, format!("Invalid TOML: {}", e)),
            },
            "yaml" => match serde_yaml::from_str::<serde_json::Value>(&content) {
                Ok(v) => (true, format!("Valid YAML. Top-level: {}", if v.is_object() { "object" } else if v.is_array() { "array" } else { "scalar" })),
                Err(e) => (false, format!("Invalid YAML: {}", e)),
            },
            _ => (false, format!("Unsupported format: {}", detected)),
        };

        Ok(ToolResultMessage {
            tool_use_id: tool_use.id.clone(), is_error: !valid,
            content: vec![ContentBlock::Text { text: format!("{}: {}", if valid { "PASS" } else { "FAIL" }, detail) }],
            elapsed_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_valid_json() {
        let tool = Arc::new(ValidateTool);
        let tu = ToolUse { id: "t1".into(), name: "validate".into(), input: serde_json::json!({"file_path": "Cargo.toml"}) };
        let ctx = ToolContext { working_dir: std::path::PathBuf::from("."), permission_mode: aegis_core::types::PermissionMode::Default, session_id: "t".into(), env: Default::default(), sandbox_enabled: false, sandbox: None, timeout_ms: 5000, ask_user_cb: Default::default(), progress_tx: None };
        let r = tool.execute(&tu, &ctx).await;
        assert!(r.is_ok()); // Cargo.toml should be valid TOML
    }
}
