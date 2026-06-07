use async_trait::async_trait;
// Screenshot tool — captures primary monitor, returns base64 PNG.
use std::sync::Arc;
use aegis_core::error::AgentResult;
use aegis_core::types::tool::{ConcurrencySafety, RiskLevel, Tool, ToolMetadata, ToolSchema};
use aegis_core::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use aegis_core::types::tool::ToolContext;
use base64::Engine;

pub struct ScreenshotTool;
impl ScreenshotTool { pub fn new() -> Self { Self } }
impl Default for ScreenshotTool { fn default() -> Self { Self } }

impl ToolMetadata for ScreenshotTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "screenshot".into(),
            description: "Capture a screenshot of the primary monitor. Returns base64-encoded PNG for visual analysis.".into(),
            prompt: "Use screenshot to see what's on screen. Returns the image as base64 data.".into(),
            input_schema: serde_json::json!({"type":"object","properties":{},"required":[]}),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

#[async_trait]
impl Tool for ScreenshotTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let monitors = xcap::Monitor::all().map_err(|e| aegis_core::error::AgentError::Internal(format!("list monitors: {e}")))?;
        let primary = monitors.first().ok_or_else(|| aegis_core::error::AgentError::Internal("no monitor found".into()))?;
        let image = primary.capture_image().map_err(|e| aegis_core::error::AgentError::Internal(format!("capture: {e}")))?;
        let mut buf = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).map_err(|e| aegis_core::error::AgentError::Internal(format!("encode: {e}")))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        let (w, h) = (image.width(), image.height());
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Screenshot {w}x{h}, {}KB base64:\n{b64}", buf.len()/1024) }], elapsed_ms: 0 })
    }
}
