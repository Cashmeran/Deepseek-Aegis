//! Aegis TUI layer — rendering, types, and terminal interaction.
//!
//! This module contains the render pipeline (`render/`) and shared types
//! (`types`) used by both the render layer and the main event loop.
//!
//! The `ui/` module (at crate level, re-exported from lib.rs) is Original
//! UI componentry used by `app/`. It is NOT used by the `tui/` render path.

pub mod render;
pub mod types;
