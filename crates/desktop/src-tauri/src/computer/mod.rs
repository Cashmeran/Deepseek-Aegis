pub mod input;
pub mod launch;
pub mod screen;
pub mod snapshot;

use std::sync::Arc;
use aegis_core::tool_system::registry::ToolRegistry;

pub fn register_all(registry: &ToolRegistry) {
    // Snapshot — primary: UIA tree (DeepSeek V4 has no multimodal, skip screenshot)
    registry.register(Arc::new(snapshot::SnapshotTool::new())).ok();
    registry.register(Arc::new(input::ClickTool::new())).ok();
    registry.register(Arc::new(input::TypeTextTool::new())).ok();
    registry.register(Arc::new(input::ScrollTool::new())).ok();
    registry.register(Arc::new(input::MoveMouseTool::new())).ok();
    registry.register(Arc::new(input::ShortcutTool::new())).ok();
    registry.register(Arc::new(launch::LaunchAppTool::new())).ok();
    // Wait / Clipboard
    registry.register(Arc::new(input::WaitTool::new())).ok();
    registry.register(Arc::new(input::ClipboardTool::new())).ok();
}
