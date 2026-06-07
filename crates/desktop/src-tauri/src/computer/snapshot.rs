// Snapshot tool — walks Windows UIA tree, returns labeled interactive elements.
// Uses PowerShell temp-file to avoid Rust format! escaping issues.
// Pattern from Windows-MCP, but simplified: script → temp file → execute.
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
        description: "Walk the Windows UI Accessibility tree and return a numbered list of interactive UI elements (buttons, inputs, links). Use labels as click/type targets. Much more reliable than screenshot for finding specific elements.".into(),
        prompt: "Use snapshot to discover UI elements on screen. Returns labeled elements that can be targeted with click/type_text. Faster and more deterministic than screenshot + visual analysis.".into(),
        input_schema: serde_json::json!({"type":"object","properties":{},"required":[]}),
    }}
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for SnapshotTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        // Write PowerShell script to temp file — avoids complex string escaping
        let script = r#"
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$r = [System.Windows.Automation.AutomationElement]::RootElement
$w = [System.Windows.Automation.TreeWalker]::new([System.Windows.Automation.Condition]::TrueCondition)
$results = @(); $c = 0; $max = 80
function W { param($e,$d)
  if ($d -gt 16 -or $c -ge $max) { return }
  try {
    $n = $e.Current
    $name = $n.Name
    $type = $n.ControlType.ProgrammaticName -replace 'ControlType\.',''
    $r = $n.BoundingRectangle
    $en = $n.IsEnabled; $v = !$n.IsOffscreen
    $skip = @('Text','Group','Pane','Window','TitleBar','ScrollBar','Thumb','Header','ToolBar','MenuBar','StatusBar','SplitButton','Separator','AppBar','Other')
    if ($v -and $en -and $name -and ($type -notin $skip)) {
      $results += [PSCustomObject]@{l=$c;name=$name;type=$type;x=[int]$r.X;y=[int]$r.Y;w=[int]$r.Width;h=[int]$r.Height}
      $c++
    }
  } catch {}
  if ($c -ge $max) { return }
  try {
    $ch = $w.GetFirstChild($e)
    while ($ch -ne $null -and $c -lt $max) { W $ch ($d+1); $ch = $w.GetNextSibling($ch) }
  } catch {}
}
W $r 0
$results | ConvertTo-Json -Depth 3 -Compress
"#;
        let tmp = std::env::temp_dir().join("aegis_uia.ps1");
        std::fs::write(&tmp, script).map_err(|e| aegis_core::error::AgentError::Internal(format!("write script: {e}")))?;

        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&tmp)
            .output()
            .map_err(|e| aegis_core::error::AgentError::Internal(format!("UIA failed (needs Windows): {e}")))?;

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
            return Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: "No interactive elements found. Try screenshot instead.".into() }], elapsed_ms: 0 });
        }
        let mut lines = vec![format!("Found {total} elements:")];
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
