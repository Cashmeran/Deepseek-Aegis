use async_trait::async_trait;
// Mouse & keyboard input tools — enigo-based.
use std::sync::Arc;
use aegis_core::error::AgentResult;
use aegis_core::types::tool::{ConcurrencySafety, RiskLevel, Tool, ToolMetadata, ToolSchema};
use aegis_core::types::message::{ContentBlock, ToolResultMessage, ToolUse};
use aegis_core::types::tool::ToolContext;
use enigo::{Coordinate, Enigo, Keyboard, Mouse, Settings};
use std::sync::Mutex;

static ENIGO: std::sync::LazyLock<Mutex<Enigo>> =
    std::sync::LazyLock::new(|| Mutex::new(Enigo::new(&Settings::default()).unwrap()));

fn parse_coords(input: &serde_json::Value) -> Option<(i32, i32)> {
    let arr = input.get("loc")?.as_array()?;
    if arr.len() != 2 { return None; }
    Some((arr[0].as_i64()? as i32, arr[1].as_i64()? as i32))
}

macro_rules! computer_tool {
    ($name:ident, $tool_name:literal, $desc:literal, $prompt:literal, $schema:expr, $risk:expr, $safe:expr) => {
        pub struct $name;
        impl $name { pub fn new() -> Self { Self } }
        impl Default for $name { fn default() -> Self { Self } }
        impl ToolMetadata for $name {
            fn schema(&self) -> ToolSchema {
                ToolSchema { name: $tool_name.into(), description: $desc.into(), prompt: $prompt.into(), input_schema: $schema }
            }
            fn risk_level(&self) -> RiskLevel { $risk }
            fn concurrency_safety(&self) -> ConcurrencySafety { $safe }
        }
    };
}

// ═══ Click ═══
computer_tool!(ClickTool, "click",
    "Click the mouse at screen coordinates [x, y].",
    "Use click to interact with UI elements at specific screen positions. Provide [x, y] coordinates.",
    serde_json::json!({"type":"object","properties":{"loc":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"[x,y] screen coordinates"},"button":{"type":"string","enum":["left","right","middle"],"default":"left"},"clicks":{"type":"integer","enum":[1,2],"default":1}},"required":["loc"]}),
    RiskLevel::Medium, ConcurrencySafety::ConcurrentUnsafe);

#[async_trait]
impl Tool for ClickTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let (x, y) = parse_coords(&tu.input).ok_or_else(|| aegis_core::error::AgentError::Internal("loc must be [x, y]".into()))?;
        let btn = match tu.input.get("button").and_then(|v| v.as_str()).unwrap_or("left") {
            "right" => enigo::Button::Right, "middle" => enigo::Button::Middle, _ => enigo::Button::Left,
        };
        let clicks = tu.input.get("clicks").and_then(|v| v.as_i64()).unwrap_or(1) as usize;
        let mut e = ENIGO.lock().unwrap();
        e.move_mouse(x, y, Coordinate::Abs).map_err(|e| aegis_core::error::AgentError::Internal(format!("move: {e}")))?;
        for _ in 0..clicks { e.button(btn, enigo::Direction::Click).map_err(|e| aegis_core::error::AgentError::Internal(format!("click: {e}")))?; }
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Clicked at ({x},{y})") }], elapsed_ms: 0 })
    }
}

// ═══ TypeText ═══
computer_tool!(TypeTextTool, "type_text",
    "Type text at the current cursor position.",
    "Click into a text field first, then use type_text to enter content. Set press_enter=true to submit.",
    serde_json::json!({"type":"object","properties":{"text":{"type":"string","description":"Text to type"},"press_enter":{"type":"boolean","default":false}},"required":["text"]}),
    RiskLevel::Medium, ConcurrencySafety::ConcurrentUnsafe);

#[async_trait]
impl Tool for TypeTextTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let text = tu.input.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let press_enter = tu.input.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut e = ENIGO.lock().unwrap();
        e.text(text).map_err(|e| aegis_core::error::AgentError::Internal(format!("type: {e}")))?;
        if press_enter { e.key(enigo::Key::Return, enigo::Direction::Click).map_err(|e| aegis_core::error::AgentError::Internal(format!("enter: {e}")))?; }
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Typed {} chars", text.len()) }], elapsed_ms: 0 })
    }
}

// ═══ Scroll ═══
computer_tool!(ScrollTool, "scroll",
    "Scroll the mouse wheel.",
    "Use scroll to navigate long content. direction: up/down, amount: number of steps (1 step ≈ 3-5 lines).",
    serde_json::json!({"type":"object","properties":{"direction":{"type":"string","enum":["up","down"],"default":"down"},"amount":{"type":"integer","default":3}},"required":[]}),
    RiskLevel::Low, ConcurrencySafety::ConcurrentUnsafe);

#[async_trait]
impl Tool for ScrollTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let amount = tu.input.get("amount").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
        let is_up = tu.input.get("direction").and_then(|v| v.as_str()).unwrap_or("down") == "up";
        let mut e = ENIGO.lock().unwrap();
        for _ in 0..amount {
            if is_up { e.scroll(1, enigo::Axis::Vertical).map_err(|e| aegis_core::error::AgentError::Internal(format!("scroll: {e}")))?; }
            else { e.scroll(-1, enigo::Axis::Vertical).map_err(|e| aegis_core::error::AgentError::Internal(format!("scroll: {e}")))?; }
        }
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Scrolled {} {} steps", if is_up {"up"} else {"down"}, amount) }], elapsed_ms: 0 })
    }
}

// ═══ MoveMouse ═══
computer_tool!(MoveMouseTool, "move_mouse",
    "Move the mouse cursor to coordinates [x, y] without clicking.",
    "Use move_mouse to hover over elements. Target coordinates from screenshot analysis.",
    serde_json::json!({"type":"object","properties":{"loc":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"[x,y] target coordinates"}},"required":["loc"]}),
    RiskLevel::Low, ConcurrencySafety::ConcurrentUnsafe);

#[async_trait]
impl Tool for MoveMouseTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let (x, y) = parse_coords(&tu.input).ok_or_else(|| aegis_core::error::AgentError::Internal("loc must be [x, y]".into()))?;
        ENIGO.lock().unwrap().move_mouse(x, y, Coordinate::Abs).map_err(|e| aegis_core::error::AgentError::Internal(format!("move: {e}")))?;
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Moved to ({x},{y})") }], elapsed_ms: 0 })
    }
}

// ═══ Shortcut ═══
computer_tool!(ShortcutTool, "shortcut",
    "Execute a keyboard shortcut like 'ctrl+c', 'alt+tab', 'win+r'.",
    "Use shortcut for system commands. Keys separated by +. Supported: ctrl, alt, shift, win, tab, enter, esc, space, backspace, delete, home, end, arrows, f1-f12, a-z.",
    serde_json::json!({"type":"object","properties":{"keys":{"type":"string","description":"e.g. 'ctrl+c', 'alt+tab', 'win+r'"}},"required":["keys"]}),
    RiskLevel::Medium, ConcurrencySafety::ConcurrentUnsafe);

#[async_trait]
impl Tool for ShortcutTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let keys_str = tu.input.get("keys").and_then(|v| v.as_str()).unwrap_or("");
        let keys: Vec<enigo::Key> = keys_str.split('+').filter_map(|k| match k.trim().to_lowercase().as_str() {
            "ctrl"|"control" => Some(enigo::Key::Control), "alt" => Some(enigo::Key::Alt),
            "shift" => Some(enigo::Key::Shift), "win"|"meta"|"cmd" => Some(enigo::Key::Meta),
            "tab" => Some(enigo::Key::Tab), "enter"|"return" => Some(enigo::Key::Return),
            "esc"|"escape" => Some(enigo::Key::Escape), "space" => Some(enigo::Key::Space),
            "backspace" => Some(enigo::Key::Backspace), "delete" => Some(enigo::Key::Delete),
            "home" => Some(enigo::Key::Home), "end" => Some(enigo::Key::End),
            "up" => Some(enigo::Key::UpArrow), "down" => Some(enigo::Key::DownArrow),
            "left" => Some(enigo::Key::LeftArrow), "right" => Some(enigo::Key::RightArrow),
            "pageup" => Some(enigo::Key::PageUp), "pagedown" => Some(enigo::Key::PageDown),
            "f1"=>Some(enigo::Key::F1),"f2"=>Some(enigo::Key::F2),"f3"=>Some(enigo::Key::F3),
            "f4"=>Some(enigo::Key::F4),"f5"=>Some(enigo::Key::F5),"f6"=>Some(enigo::Key::F6),
            "f7"=>Some(enigo::Key::F7),"f8"=>Some(enigo::Key::F8),"f9"=>Some(enigo::Key::F9),
            "f10"=>Some(enigo::Key::F10),"f11"=>Some(enigo::Key::F11),"f12"=>Some(enigo::Key::F12),
            s if s.len()==1 => Some(enigo::Key::Unicode(s.chars().next().unwrap())),
            _ => None,
        }).collect();
        let mut e = ENIGO.lock().unwrap();
        for k in &keys { e.key(*k, enigo::Direction::Press).map_err(|e| aegis_core::error::AgentError::Internal(format!("press: {e}")))?; }
        for k in keys.iter().rev() { e.key(*k, enigo::Direction::Release).map_err(|e| aegis_core::error::AgentError::Internal(format!("release: {e}")))?; }
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Pressed {keys_str}") }], elapsed_ms: 0 })
    }
}

// ═══ Wait ═══
computer_tool!(WaitTool, "wait",
    "Pause execution for the specified duration in seconds.",
    "Use wait when you need to: let an application launch/load, wait for a UI animation to complete, allow a page to render, or pause between rapid actions. Duration 1-10 seconds.",
    serde_json::json!({"type":"object","properties":{"duration":{"type":"integer","default":1,"minimum":1,"maximum":10,"description":"Seconds to wait (1-10)"}},"required":[]}),
    RiskLevel::Low, ConcurrencySafety::ConcurrentSafe);

#[async_trait]
impl Tool for WaitTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let secs = tu.input.get("duration").and_then(|v| v.as_i64()).unwrap_or(1).max(1).min(10) as u64;
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Waited {secs}s") }], elapsed_ms: 0 })
    }
}

// ═══ Clipboard ═══
computer_tool!(ClipboardTool, "clipboard",
    "Read from or write to the system clipboard.",
    "Use clipboard to get text content from the clipboard (read), or set clipboard text (write with 'text' param).",
    serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["read","write"],"default":"read","description":"read=get clipboard text, write=set clipboard text"},"text":{"type":"string","description":"Text to write to clipboard (required when action=write)"}},"required":["action"]}),
    RiskLevel::Low, ConcurrencySafety::ConcurrentSafe);

#[async_trait]
impl Tool for ClipboardTool {
    async fn execute(self: Arc<Self>, tu: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let action = tu.input.get("action").and_then(|v| v.as_str()).unwrap_or("read");
        match action {
            "write" => {
                let text = tu.input.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let mut ctx = arboard::Clipboard::new().map_err(|e| aegis_core::error::AgentError::Internal(format!("clipboard: {e}")))?;
                ctx.set_text(text).map_err(|e| aegis_core::error::AgentError::Internal(format!("set: {e}")))?;
                Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: "Clipboard written".into() }], elapsed_ms: 0 })
            }
            _ => {
                let mut ctx = arboard::Clipboard::new().map_err(|e| aegis_core::error::AgentError::Internal(format!("clipboard: {e}")))?;
                let text = ctx.get_text().unwrap_or_default();
                Ok(ToolResultMessage { tool_use_id: tu.id.clone(), is_error: false, content: vec![ContentBlock::Text { text: format!("Clipboard: {text}") }], elapsed_ms: 0 })
            }
        }
    }
}
