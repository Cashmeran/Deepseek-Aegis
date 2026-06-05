//! Shared types used by the TUI render layer and the main event loop.

use std::time::{Duration, Instant};

// ── Frame rate ──────────────────────────────────────────────────────

pub const FRAME_MS: u64 = 16;

pub struct Throttle {
    pub last: Instant,
    pub force: bool,
}

impl Throttle {
    pub fn new() -> Self {
        Self { last: Instant::now() - Duration::from_millis(FRAME_MS), force: false }
    }
    pub fn should(&self) -> bool {
        self.force || self.last.elapsed().as_millis() as u64 >= FRAME_MS
    }
    pub fn mark(&mut self) {
        self.last = Instant::now();
        self.force = false;
    }
}

// ── Messages ────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum Msg {
    User(String),
    Asst { text: String, think: String },
    Tool { id: String, name: String, done: bool, ok: bool, detail: String, elapsed_ms: u64 },
    System(String),
}

// ── Dialogs ─────────────────────────────────────────────────────────

pub struct AskDialog {
    pub question: String,
    pub header: String,
    pub options: Vec<String>,
    pub selected: usize,
    pub custom_input: String,
    pub custom_cursor: usize,
    pub in_custom: bool,
}

pub struct ModelDialog {
    pub models: Vec<(&'static str, &'static str)>,
    pub model_idx: usize,
    pub effort_idx: usize,
}

pub const EFFORTS: &[&str] = &["off", "high", "max"];
pub const MODELS: &[(&str, &str)] = &[
    ("deepseek-v4-pro", "DeepSeek V4 Pro"),
    ("deepseek-v4-flash", "DeepSeek V4 Flash"),
];

pub struct SkillDialog {
    pub skills: Vec<(String, String)>,
    pub skill_idx: usize,
}

pub struct SessionDialog {
    pub sessions: Vec<(String, u64, String, String)>,
    pub session_idx: usize,
}
