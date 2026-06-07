// Snapshot — walks Windows UIA tree via PowerShell, returns labeled interactive elements.
// Script loaded from snapshot.ps1 at compile time via include_str!, avoiding all string escaping.
use std::sync::Arc;
use aegis_core::error::AgentResult;
use aegis_core::types::tool::{ConcurrencySafety, RiskLevel, Tool, ToolMetadata, ToolSchema};
use aegis_core::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use aegis_core::types::tool::ToolContext;
use async_trait::async_trait;

pub struct SnapshotTool;
impl SnapshotTool { pub fn new() -> Self { Self } }
impl Default for SnapshotTool { fn default() -> Self { Self } }

impl ToolMetadata for SnapshotTool {
    fn schema(&self) -> ToolSchema { ToolSchema {
        name: "snapshot".into(),
        description: "Walk the Windows UI Accessibility tree and return a numbered list of interactive UI elements".into(),
        prompt: "Use snapshot to discover UI elements on screen. Returns labeled list like [0] Button \"OK\" @ (x,y) w*h. Much more reliable than screenshot.".into(),
        input_schema: serde_json::json!({"type":"object","properties":{},"required":[]}),
    }}
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for SnapshotTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let script = include_str!("snapshot.ps1");
        // Use unique name to avoid stale cached scripts
        let tmp = std::env::temp_dir().join(format!("aegis_snap_{}.ps1", std::process::id()));
        std::fs::write(&tmp, script).map_err(|e| aegis_core::error::AgentError::Internal(format!("write: {e}")))?;

        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&tmp)
            .output()
            .map_err(|e| aegis_core::error::AgentError::Internal(format!("snapshot: {e}")))?;

        let _ = std::fs::remove_file(&tmp);

        if !out.status.success() {
            return Err(aegis_core::error::AgentError::Internal(
                format!("UIA error: {}", String::from_utf8_lossy(&out.stderr))
            ));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let elements: Vec<serde_json::Value> = serde_json::from_str(stdout.trim()).unwrap_or_default();
        let total = elements.len();
        if total == 0 {
            return Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: "No interactive elements found. Try screenshot.".into() }], elapsed_ms: 0 });
        }
        let mut lines = vec![format!("Found {total} interactive elements:")];
        for el in &elements {
            let l = el["l"].as_i64().unwrap_or(0);
            let n = el["name"].as_str().unwrap_or("?");
            let t = el["type"].as_str().unwrap_or("?");
            let x = el["x"].as_i64().unwrap_or(0);
            let y = el["y"].as_i64().unwrap_or(0);
            let w = el["w"].as_i64().unwrap_or(0);
            let h = el["h"].as_i64().unwrap_or(0);
            lines.push(format!("  [{l}] {t} \"{n}\" @ ({x},{y}) {w}x{h}"));
        }
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: lines.join("\n") }], elapsed_ms: 0 })
    }
}
