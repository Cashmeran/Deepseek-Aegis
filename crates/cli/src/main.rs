//! Minimal working TUI for aegis.
//!
//! Layout:
//! ┌──────────────────────────────┐
//! │  Messages area               │
//! ├──────────────────────────────┤
//! │ ┌ input box with borders ──┐ │
//! │ │ > user text            █ │ │
//! │ └──────────────────────────┘ │
//! │ hints        [model][effort] │
//! └──────────────────────────────┘

use std::io::{self, Write, stdout};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use aegis_core::agent::system_prompt::SystemPromptBuilder;
use aegis_core::agent::AgentLoop;
use aegis_core::error::AgentResult;
use aegis_core::llm::client::StreamEvent;
use aegis_core::llm::deepseek::DeepSeekClient;
use aegis_core::tool_system::registry::ToolRegistry;
use aegis_core::types::config::AgentConfig;
use aegis_core::types::{ConcurrencySafety, ContentBlock, RiskLevel, Tool, ToolContext, ToolMetadata, ToolResultMessage, ToolSchema, ToolUse};
use aegis_code_graph::GraphStore as CodeGraphStore;
use aegis_memory::MemoryStore;
use async_trait::async_trait;

// Global channel for ask_user response (agent callback → TUI)
type AskSender = std::sync::mpsc::SyncSender<String>;
static ASK_RESPONSE: LazyLock<Mutex<Option<AskSender>>> = LazyLock::new(|| Mutex::new(None));

// ── App state ───────────────────────────────────────────────────

pub struct App {
    input: String,
    cursor_byte: usize,
    input_scroll_x: u16,
    messages: Vec<Msg>,
    scroll: usize,
    viewport_height: usize,
    agent_tx: mpsc::UnboundedSender<String>,
    stream_rx: mpsc::UnboundedReceiver<StreamEvent>,
    quit: bool,
    model: String,
    mode: String,
    reasoning_effort: String,
    tokens_in: u64,
    tokens_out: u64,
    cache_tokens: u64,
    cost: f64,
    running: bool,
    turn_start: Option<Instant>,
    last_turn_ms: u64,
    /// Per-turn token counters (reset on each submit, shown live).
    turn_tokens_in: u64,
    turn_tokens_out: u64,
    turn_tokens_cache: u64,
    last_call_cache_pct: f64,
    last_assist_idx: Option<usize>,
    sel_start: Option<usize>,
    sel_end: Option<usize>,
    msg_area_y: u16,
    msg_area_h: u16,
    lines_buf: Vec<String>,
    input_y: u16,
    input_h: u16,
    dialog: Option<AskDialog>,
    model_dialog: Option<ModelDialog>,
    skill_dialog: Option<SkillDialog>,
    session_dialog: Option<SessionDialog>,
    skill_registry: Option<Arc<aegis_core::skills::SkillRegistry>>,
    sandbox_enabled: bool,
    shared_mode: Option<Arc<std::sync::RwLock<aegis_core::types::tool::ExecutionMode>>>,
    shared_config: Option<Arc<std::sync::RwLock<AgentConfig>>>,
    paste_buf: Vec<String>, // index → pasted content
    paste_counter: usize,
}

impl App {
    fn expand_paste_refs(&self, text: &str) -> String {
        // Replace each paste reference marker with the stored content.
        // Marker format: ⟨PASTE:N⟩ where N is the 1-based index.
        let mut result = text.to_string();
        for (idx, content) in self.paste_buf.iter().enumerate() {
            let marker = format!("⟨PASTE:{}⟩", idx + 1);
            result = result.replace(&marker, content);
        }
        result
    }

    fn submit(&mut self) {
        let all = self.input.clone();
        if all.trim().is_empty() { return; }
        let mut text = self.expand_paste_refs(&all);
        // If any paste markers remain unexpanded (e.g. user edited the marker),
        // keep them as-is and warn via the footer instead of silently failing
        if text.contains("⟨PASTE:") {
            // Unresolved markers: the stored content was never captured — keep marker as literal text
            // This happens when the user types "⟨PASTE:" manually (not via actual paste)
            text = text.replace("⟨PASTE:", "[PASTE:");
            text = text.replace("⟩", "]");
        }
        self.paste_buf.clear();
        self.paste_counter = 0;
        self.input.clear(); self.cursor_byte = 0; self.input_scroll_x = 0;
        self.messages.push(Msg::User(text.clone()));
        self.scroll_to_bottom();
        self.running = false;
        let _ = self.agent_tx.send(text);
        self.running = true;
        self.turn_start = Some(Instant::now());
        self.last_turn_ms = 0;
        self.turn_tokens_in = 0;
        self.turn_tokens_out = 0;
        self.turn_tokens_cache = 0;
    }

    fn insert(&mut self, ch: char) { self.input.insert(self.cursor_byte, ch); self.cursor_byte += ch.len_utf8(); }
    fn backspace(&mut self) { if self.cursor_byte > 0 { let p = self.input[..self.cursor_byte].char_indices().last().map(|(i,_)| i).unwrap_or(0); self.input.replace_range(p..self.cursor_byte, ""); self.cursor_byte = p; } }
    fn delete_forward(&mut self) { if self.cursor_byte < self.input.len() { let end = self.input[self.cursor_byte..].chars().next().map(|c| self.cursor_byte + c.len_utf8()).unwrap_or(self.cursor_byte); self.input.drain(self.cursor_byte..end); } }
    fn cursor_left(&mut self) { if self.cursor_byte > 0 { self.cursor_byte = self.input[..self.cursor_byte].char_indices().last().map(|(i,_)| i).unwrap_or(0); } }
    fn cursor_right(&mut self) { if self.cursor_byte < self.input.len() { self.cursor_byte = self.input[self.cursor_byte..].chars().next().map(|c| self.cursor_byte + c.len_utf8()).unwrap_or(self.cursor_byte); } }
    fn cursor_home(&mut self) { self.cursor_byte = 0; }
    fn cursor_end(&mut self) { self.cursor_byte = self.input.len(); }
    fn scroll_to_bottom(&mut self) { self.scroll = self.total_lines().saturating_sub(self.viewport_height); }
    fn scroll_up(&mut self, n: usize) { self.scroll = self.scroll.saturating_sub(n); }
    fn scroll_down(&mut self, n: usize) { self.scroll = (self.scroll + n).min(self.total_lines().saturating_sub(1)); }
    fn total_lines(&self) -> usize { self.lines_buf.len() }
    fn at_bottom(&self) -> bool { self.scroll >= self.total_lines().saturating_sub(self.viewport_height) }

    fn handle_stream(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::TextDelta(delta) => { self.append_or_create_assistant(&delta, ""); }
            StreamEvent::ThinkingDelta(delta) => { self.append_or_create_assistant("", &delta); }
            StreamEvent::ToolUseStart { id: _, name, input } => {
                let detail = if name == "ask_user" {
                    "等待回复…".to_string()
                } else {
                    serde_json::to_string(&input).unwrap_or_default().chars().take(100).collect()
                };
                self.messages.push(Msg::Tool { name, done: false, ok: true, detail, elapsed_ms: 0 });
                self.last_assist_idx = None;
            }
            StreamEvent::ToolResult { id: _, name, is_error, output, elapsed_ms } => {
                // Iterate backward to find the most recent undone tool — last_mut() may point
                // at a text message added between tool-start and tool-result.
                for msg in self.messages.iter_mut().rev() {
                    if let Msg::Tool { done, ok, detail, elapsed_ms: tool_elapsed, .. } = msg {
                        if !*done {
                            *done = true;
                            *ok = !is_error;
                            *tool_elapsed = elapsed_ms;
                            let summary: String = output.chars().take(5000).collect();
                            if name == "ask_user" {
                                *detail = format!("用户回复: {summary}");
                            } else {
                                *detail = format!("{} | {}ms | {}", name, elapsed_ms, summary);
                            }
                            break;
                        }
                    }
                }
            }
            StreamEvent::AskUser { question, header, options } => {
                self.dialog = Some(AskDialog {
                    question,
                    header,
                    options: options.into_iter().map(|o| o.label).collect(),
                    selected: 0,
                    custom_input: String::new(),
                    custom_cursor: 0,
                    in_custom: false,
                });
            }
            StreamEvent::ToolProgress { tool_use_id: _, line } => {
                if let Some(Msg::Tool { detail, .. }) = self.messages.last_mut() {
                    *detail = format!("{}", line.trim().chars().take(500).collect::<String>());
                }
            }
            StreamEvent::Done(resp) => {
                // Only the "end_turn" Done (manually sent by spawn_agent) stops the timer.
                // Intermediate Dones from tool-calling LLM turns keep running.
                if resp.stop_reason.as_deref() == Some("__turn_complete__") {
                    self.running = false;
                }
                self.last_turn_ms = self.turn_start.map_or(0, |s| s.elapsed().as_millis() as u64);
                self.tokens_in += resp.usage.input_tokens;
                self.tokens_out += resp.usage.output_tokens;
                self.cache_tokens += resp.usage.cache_read_tokens;
                self.turn_tokens_in += resp.usage.input_tokens;
                self.turn_tokens_out += resp.usage.output_tokens;
                self.turn_tokens_cache += resp.usage.cache_read_tokens;
                if resp.usage.input_tokens > 0 || resp.usage.cache_read_tokens > 0 {
                    let total = resp.usage.input_tokens + resp.usage.cache_read_tokens;
                    self.last_call_cache_pct = if total > 0 {
                        resp.usage.cache_read_tokens as f64 * 100.0 / total as f64
                    } else { 0.0 };
                }
                self.cost += (resp.usage.input_tokens as f64 * 0.14
                    + resp.usage.output_tokens as f64 * 0.28) / 1_000_000.0;
                if let Some(Msg::Tool { done, .. }) = self.messages.last_mut() { *done = true; }
                self.last_assist_idx = None;
            }
        }
        // Scroll handled in render — lines_buf is stale here
    }

    fn append_or_create_assistant(&mut self, text: &str, think: &str) {
        match self.last_assist_idx {
            Some(idx) => {
                let msg = &mut self.messages[idx];
                if let Msg::Asst { text: t, think: th } = msg { t.push_str(text); th.push_str(think); }
            }
            None => {
                self.messages.push(Msg::Asst { text: text.to_string(), think: think.to_string() });
                self.last_assist_idx = Some(self.messages.len() - 1);
            }
        }
    }

    fn cycle_mode(&mut self) {
        let modes = ["chat", "plan", "default", "yolo"];
        let idx = modes.iter().position(|m| *m == self.mode).unwrap_or(0);
        self.mode = modes[(idx + 1) % modes.len()].to_string();
        let mode_str = self.mode.clone();
        // Sync to agent
        if let Some(ref mode_ref) = self.shared_mode {
            let new_mode = match mode_str.as_str() {
                "chat" => aegis_core::types::tool::ExecutionMode::Chat,
                "plan" => aegis_core::types::tool::ExecutionMode::Plan,
                "yolo" => aegis_core::types::tool::ExecutionMode::Yolo,
                _ => aegis_core::types::tool::ExecutionMode::Default,
            };
            *mode_ref.write().unwrap() = new_mode;
        }
        self.messages.push(Msg::System(format!("Mode: {mode_str}")));
    }

    fn toggle_config_bool(&self, key: &str) -> Option<String> {
        if let Some(ref cfg) = self.shared_config {
            let mut c = cfg.write().unwrap();
            match key {
                "thinking" => { c.thinking_enabled = !c.thinking_enabled; Some(format!("Thinking: {}", if c.thinking_enabled { "ON" } else { "OFF" })) }
                "web" => { c.web_search_enabled = !c.web_search_enabled; Some(format!("Web Search: {}", if c.web_search_enabled { "ON" } else { "OFF" })) }
                "verify" => { c.verify_before_output = !c.verify_before_output; Some(format!("Verify: {}", if c.verify_before_output { "ON" } else { "OFF" })) }
                "auto" => { c.auto_model_routing = !c.auto_model_routing; Some(format!("Auto Routing: {}", if c.auto_model_routing { "ON" } else { "OFF" })) }
                "snapshot" | "snap" => { c.snapshots_enabled = !c.snapshots_enabled; Some(format!("Snapshots: {}", if c.snapshots_enabled { "ON" } else { "OFF" })) }
                _ => None
            }
        } else { None }
    }

    const VALID_COMMANDS: &[&str] = &[
        "clear", "model", "skill", "mcp", "thinking", "verify", "snap", "snapshot", "sandbox",
        "compact", "diff", "export", "review", "rollback", "resume",
        "stats", "status", "context", "help", "goal",
    ];

    fn command_matches(prefix: &str) -> Vec<&'static str> {
        if prefix.is_empty() { return Vec::new(); }
        let p = prefix.to_lowercase();
        App::VALID_COMMANDS.iter().filter(|c| c.starts_with(&p)).copied().collect()
    }

    fn is_valid_command(name: &str) -> bool {
        App::VALID_COMMANDS.contains(&name)
    }

    fn handle_slash_command(&mut self, cmd: &str) -> Option<String> {
        let parts: Vec<&str> = cmd[1..].splitn(2, ' ').collect();
        let name = parts[0];
        let arg = parts.get(1).unwrap_or(&"");
        match name {
            "clear" => { self.messages.clear(); self.scroll = 0; self.last_assist_idx = None; None }
            "model" => {
                let current_effort = self.shared_config.as_ref()
                    .and_then(|c| Some(c.read().unwrap().reasoning_effort.clone()))
                    .unwrap_or_else(|| "max".into());
                let effort_idx = EFFORTS.iter().position(|e| *e == current_effort).unwrap_or(2);
                let model_idx = MODELS.iter().position(|m| m.0 == self.model).unwrap_or(0);
                self.model_dialog = Some(ModelDialog { models: MODELS.to_vec(), model_idx, effort_idx });
                None
            }
            "skill" => {
                if let Some(ref reg) = self.skill_registry {
                    let arg = arg.trim();
                    if arg.is_empty() {
                        // No args: show skill picker dialog
                        let skills: Vec<(String, String)> = reg.list().into_iter()
                            .map(|(n, d, _)| (n, d)).collect();
                        if skills.is_empty() {
                            Some("No skills loaded. Place SKILL.md files in .agent/skills/<name>/".into())
                        } else {
                            self.skill_dialog = Some(SkillDialog { skills, skill_idx: 0 });
                            None
                        }
                    } else {
                        // Direct invocation: inject skill prompt
                        if let Some(skill) = reg.get(arg) {
                            let injection = skill.to_prompt_injection();
                            self.messages.push(Msg::System(format!("Loaded skill: {}", skill.name)));
                            let _ = self.agent_tx.send(format!("__SKILL__\n{}", injection));
                            Some(format!("Skill '{}' activated. It will guide the next response.", skill.name))
                        } else {
                            let skill_list = reg.list();
                            let available: Vec<&str> = skill_list.iter().map(|(n, _, _)| n.as_str()).collect();
                            Some(format!("Skill '{}' not found. Try /skill for a list.\nAvailable: {}", arg, available.join(", ")))
                        }
                    }
                } else {
                    Some("Skill system not initialized.".into())
                }
            }
            "thinking" | "verify" | "snapshot" | "snap" => self.toggle_config_bool(name),
            "status" => {
                let cfg = self.shared_config.as_ref().map(|c| c.read().unwrap().clone());
                let thinking = cfg.as_ref().map_or(true, |c| c.thinking_enabled);
                Some(format!("Mode: {} | Model: {} | Thinking: {} | Tokens: {} in / {} out | Cost: {}",
                    self.mode, self.model,
                    if thinking { "ON" } else { "OFF" },
                    fmt_tokens(self.tokens_in), fmt_tokens(self.tokens_out), fmt_cost(self.cost)))
            }
            "context" => { Some(format!("Context: {}/{} tokens used", fmt_tokens(self.tokens_in), fmt_tokens(1_048_576u64))) }
            "compact" => {
                let before = self.tokens_in;
                let _ = self.agent_tx.send("__COMPACT__".to_string());
                Some(format!("Compacting context (was {} tokens)...", fmt_tokens(before)))
            }
            "diff" => {
                let stat = std::process::Command::new("git").args(["diff", "--stat"]).output();
                let diff = std::process::Command::new("git").args(["diff"]).output();
                match (stat, diff) {
                    (Ok(s), Ok(d)) => {
                        let stat_out = String::from_utf8_lossy(&s.stdout);
                        let diff_out = String::from_utf8_lossy(&d.stdout);
                        if stat_out.trim().is_empty() { Some("No changes (working tree clean)".into()) }
                        else { Some(format!("{stat_out}\n\n---\n\n{}", diff_out.lines().take(100).collect::<Vec<_>>().join("\n"))) }
                    }
                    _ => Some("Not a git repository".into())
                }
            }
            "export" => {
                let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let path = format!(".agent/export-{}.md", ts);
                let mut content = String::from("# Aegis Session Export\n\n");
                for msg in &self.messages {
                    match msg {
                        Msg::User(t) => { content.push_str(&format!("**▸ You**\n\n{t}\n\n")); }
                        Msg::Asst { text, think: _ } => { content.push_str(&format!("*** Aegis**\n\n{text}\n\n")); }
                        Msg::Tool { name, ok, detail, .. } => {
                            let icon = if *ok { "+" } else { "x" };
                            content.push_str(&format!("*{icon} `{name}`* — {detail}\n\n"));
                        }
                        Msg::System(t) => { content.push_str(&format!("*System: {t}*\n\n")); }
                    }
                }
                std::fs::write(&path, &content).ok();
                Some(format!("Exported {} messages to {}", self.messages.len(), path))
            }
            "resume" => {
                let arg = arg.trim();
                // With argument: load a specific session
                if !arg.is_empty() {
                    let path = if arg.starts_with(".agent/") {
                        arg.to_string()
                    } else {
                        format!(".agent/sessions/{}", arg)
                    };
                    let path = if path.ends_with(".json") { path } else { format!("{}.json", path) };
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                            let input = v.get("user_input").and_then(|s| s.as_str()).unwrap_or("");
                            // Load messages from session
                            if let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) {
                                for msg in msgs {
                                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                    match role {
                                        "user" => self.messages.push(Msg::User(content.to_string())),
                                        "assistant" => self.messages.push(Msg::Asst { text: content.to_string(), think: String::new() }),
                                        _ => self.messages.push(Msg::System(content.to_string())),
                                    }
                                }
                            }
                            self.scroll_to_bottom();
                            // Send resume command to agent
                            let _ = self.agent_tx.send(format!("__RESUME__{}", path));
                            self.running = true;
                            self.turn_start = Some(Instant::now());
                            return Some(format!("Session loaded. Resuming: {}", input.chars().take(100).collect::<String>()));
                        }
                    }
                    return Some(format!("Session '{}' not found.", arg));
                }
                // No argument: show picker
                let mut sessions = Vec::new();
                if let Ok(entries) = std::fs::read_dir(".agent/sessions") {
                    for e in entries.filter_map(|e| e.ok()) {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".json") {
                            if let Ok(data) = std::fs::read_to_string(e.path()) {
                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                                    let turn = v.get("turn").and_then(|t| t.as_u64()).unwrap_or(0);
                                    let input = v.get("user_input").and_then(|s| s.as_str()).unwrap_or("");
                                    let ts = v.get("timestamp").and_then(|s| s.as_str()).unwrap_or("");
                                    sessions.push((name, turn, input.to_string(), ts.to_string()));
                                }
                            }
                        }
                    }
                }
                sessions.sort_by(|a, b| b.1.cmp(&a.1));
                if sessions.is_empty() {
                    Some("No saved sessions yet. Sessions are auto-saved after each turn.".into())
                } else {
                    self.session_dialog = Some(SessionDialog { sessions, session_idx: 0 });
                    None
                }
            }
            "review" => {
                let diff = std::process::Command::new("git").args(["diff"]).output();
                match diff {
                    Ok(d) => {
                        let diff_text = String::from_utf8_lossy(&d.stdout).to_string();
                        if diff_text.trim().is_empty() {
                            Some("No changes to review (working tree clean)".into())
                        } else {
                            let truncated: String = diff_text.lines().take(200).collect::<Vec<_>>().join("\n");
                            let prompt = format!("Review these uncommitted changes for bugs, security issues, and code quality:\n```diff\n{}\n```\nUse /rollback to revert all changes if needed.", truncated);
                            self.messages.push(Msg::User(prompt.clone()));
                            let _ = self.agent_tx.send(prompt);
                            self.running = true;
                            self.turn_start = Some(Instant::now());
                            None
                        }
                    }
                    _ => Some("Not a git repository".into())
                }
            }
            "rollback" => {
                let diff = std::process::Command::new("git").args(["diff", "--stat"]).output();
                match diff {
                    Ok(d) => {
                        let stat = String::from_utf8_lossy(&d.stdout).to_string();
                        if stat.trim().is_empty() {
                            Some("Nothing to rollback (working tree clean)".into())
                        } else {
                            let files: Vec<String> = stat.lines().filter_map(|l| {
                                let parts: Vec<&str> = l.split('|').collect();
                                if parts.len() >= 1 { Some(parts[0].trim().to_string()) } else { None }
                            }).collect();
                            let prompt = format!("Rollback: revert the following {} changed files to their last committed state. Use git checkout for each file:\n{}", files.len(), files.join("\n"));
                            self.messages.push(Msg::User(prompt.clone()));
                            let _ = self.agent_tx.send(prompt);
                            self.running = true;
                            self.turn_start = Some(Instant::now());
                            None
                        }
                    }
                    _ => Some("Not a git repository".into())
                }
            }
            "goal" => {
                if arg.is_empty() { Some("Usage: /goal <objective> | <criterion1>, <criterion2>, ...".into()) }
                else {
                    let parts: Vec<&str> = arg.split('|').collect();
                    let objective = parts.get(0).unwrap_or(&"").trim();
                    let criteria: Vec<String> = if parts.len() > 1 {
                        parts[1].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    } else { vec![] };
                    if objective.is_empty() { Some("Goal requires an objective".into()) }
                    else {
                        let mut contract = aegis_core::agent::SprintContract::new(objective.to_string());
                        for c in &criteria {
                            contract.acceptance_criteria.push(aegis_core::agent::AcceptanceCriterion {
                                description: c.clone(),
                                verification_command: String::new(),
                                expected_exit_code: 0,
                                expected_output_contains: None,
                            });
                        }
                        let criteria_str = if criteria.is_empty() { "(auto-judge)".to_string() } else { criteria.join(", ") };
                        // Send goal to agent via special message
                        let prompt = format!("__GOAL__{}\nCriteria: {}", objective, criteria_str);
                        self.messages.push(Msg::User(prompt.clone()));
                        let _ = self.agent_tx.send(prompt);
                        self.running = true;
                        self.turn_start = Some(Instant::now());
                        Some(format!("Goal set: {}. Criteria: {}. Agent will auto-verify completion.", objective, criteria_str))
                    }
                }
            }
            "stats" => {
                let msgs = self.messages.len();
                let users = self.messages.iter().filter(|m| matches!(m, Msg::User(_))).count();
                let tools = self.messages.iter().filter(|m| matches!(m, Msg::Tool { .. })).count();
                let tokens_total = self.tokens_in + self.tokens_out;
                let cache_pct = if self.tokens_in > 0 { self.cache_tokens * 100 / self.tokens_in } else { 0 };
                let _elapsed = self.turn_start.map_or(0, |s| s.elapsed().as_secs());
                Some(format!(
                    "Session Stats:\n  Messages: {msgs} ({users} user, {} asst, {tools} tool)\n  Tokens: {} in / {} out ({} total)\n  Cache hit: {}%\n  Cost: {}\n  Mode: {}\n  Model: {}",
                    msgs - users - tools,
                    fmt_tokens(self.tokens_in), fmt_tokens(self.tokens_out), fmt_tokens(tokens_total),
                    cache_pct,
                    fmt_cost(self.cost),
                    self.mode, self.model
                ))
            }
            "sandbox" => {
                self.sandbox_enabled = !self.sandbox_enabled;
                Some(format!("Sandbox: {}", if self.sandbox_enabled { "ON (process isolation)" } else { "OFF" }))
            }
            "mcp" => {
                let ok = std::process::Command::new("cargo")
                    .args(["check", "--message-format=short"])
                    .current_dir(".").output().is_ok();
                if ok {
                    Some("MCP system active. Configure servers in .mcp.json:\n".to_string()
                        + &aegis_mcp::generate_default_mcp_json())
                } else {
                    Some("MCP system active. Use list_mcp_resources tool to discover servers.".into())
                }
            }
            "help" => {
                Some("/clear /model /skill /mcp /thinking /verify /snap /sandbox /compact /diff /export /review /rollback /resume /stats /status /context /help\n!cmd for shell | @file to reference | Shift+Tab cycle mode".into())
            }
            _ => { Some(format!("Unknown command: /{name}. Try /help")) }
        }
    }

    fn run_bash(&self, cmd: &str) -> String {
        let shell_cmd = &cmd[1..]; // strip !
        match std::process::Command::new(if cfg!(windows) { "powershell" } else { "bash" })
            .arg(if cfg!(windows) { "-Command" } else { "-c" })
            .arg(shell_cmd)
            .output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut r = String::new();
                if !stdout.trim().is_empty() { r.push_str(&stdout); }
                if !stderr.trim().is_empty() { r.push_str(&format!("\n[stderr]\n{stderr}")); }
                if r.is_empty() { format!("(exit {})", out.status.code().unwrap_or(-1)) } else { r }
            }
            Err(e) => format!("Failed: {e}"),
        }
    }

    fn file_completions(prefix: &str) -> Vec<String> {
        let search = if prefix.is_empty() { "." } else { prefix };
        let dir = std::path::Path::new(search);
        let (base_dir, file_prefix) = if search.ends_with('/') || dir.is_dir() {
            (dir.to_path_buf(), String::new())
        } else {
            (dir.parent().unwrap_or(std::path::Path::new(".")).to_path_buf(),
             dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
        };
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
                if name.starts_with(&file_prefix) {
                    let full = base_dir.join(&name);
                    results.push(format!("@{}", full.display()));
                }
            }
        }
        results.truncate(20);
        results
    }
}

// ── Terminal setup ──────────────────────────────────────────────

struct TermGuard;
impl TermGuard {
    fn enter() -> io::Result<Self> {
        let mut out = stdout();
        out.execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        out.execute(crossterm::event::EnableMouseCapture)?;
        Ok(Self)
    }
}
impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = out.execute(crossterm::event::DisableMouseCapture);
        let _ = disable_raw_mode();
        let _ = out.execute(LeaveAlternateScreen);
        let _ = out.flush();
    }
}

fn install_panic_hook() {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut out = stdout();
        let _ = out.execute(LeaveAlternateScreen);
        let _ = out.execute(crossterm::event::DisableMouseCapture);
        let _ = out.flush();
        hook(info);
    }));
}

// ── Lazy code graph tool (per-call connection) ──────────────────

struct LazyCodeGraphTool {
    db_path: std::path::PathBuf,
}

#[async_trait]
impl Tool for LazyCodeGraphTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let file_path = tool_use.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        if file_path.is_empty() {
            return Err(aegis_core::AgentError::ToolValidationError {
                tool: "get_architectural_context".into(), errors: "file_path is required".into(),
            });
        }
        let start = std::time::Instant::now();
        // Open a fresh connection per call — no Mutex, no contention
        match <aegis_code_graph::SqliteGraphStore as aegis_code_graph::GraphStore>::open(&self.db_path) {
            Ok(store) => {
                let cwd = std::env::current_dir().unwrap_or_default();
                let abs_path = cwd.join(file_path);
                let path_str = abs_path.to_string_lossy().replace('\\', "/");
                let text = aegis_code_graph::get_architectural_context(&store, &path_str)
                    .unwrap_or_else(|e| format!("Not indexed: {path_str} ({e})"));
                let truncated: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
                Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text { text: truncated }],
                    elapsed_ms: start.elapsed().as_millis() as u64 })
            }
            Err(e) => Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: format!("DB error: {e}") }],
                elapsed_ms: start.elapsed().as_millis() as u64 }),
        }
    }
}

impl ToolMetadata for LazyCodeGraphTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "get_architectural_context".into(),
            description: "Returns 1-hop architectural context: imports, callers, callees, inheritance".into(),
            prompt: "Use BEFORE editing any file to understand its relationships.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "file_path": { "type": "string", "description": "Path to source file" } },
                "required": ["file_path"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

// ── Codebase scan tool (on-demand, AI decides when to index) ──────

struct ScanCodebaseTool {
    db_path: std::path::PathBuf,
}

#[async_trait]
impl Tool for ScanCodebaseTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let start = std::time::Instant::now();
        let dir = tool_use.input.get("directory").and_then(|v| v.as_str())
            .map(|d| std::path::PathBuf::from(d))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        match aegis_code_graph::SqliteGraphStore::open(&self.db_path) {
            Ok(store) => {
                let store: Arc<dyn aegis_code_graph::GraphStore> = Arc::new(store);
                let lang_registry = Arc::new(aegis_code_graph::create_default_registry());
                let parser = Arc::new(aegis_code_graph::CodeParser::new(lang_registry.clone()));
                let indexer = aegis_code_graph::IncrementalIndexer::new(store, parser, lang_registry);
                match indexer.full_scan(&dir) {
                    Ok(r) => {
                        let _ = rusqlite::Connection::open(&self.db_path)
                            .and_then(|c| c.pragma_update(None, "wal_checkpoint", "TRUNCATE"));
                        Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false,
                            content: vec![ContentBlock::Text {
                                text: format!("Indexed {} files in {}ms.", r.total_files, r.elapsed_ms)
                            }],
                            elapsed_ms: start.elapsed().as_millis() as u64 })
                    }
                    Err(e) => Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: true,
                        content: vec![ContentBlock::Text { text: format!("Scan failed: {e}") }],
                        elapsed_ms: start.elapsed().as_millis() as u64 }),
                }
            }
            Err(e) => Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: format!("DB error: {e}") }],
                elapsed_ms: start.elapsed().as_millis() as u64 }),
        }
    }
}

impl ToolMetadata for ScanCodebaseTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "scan_codebase".into(),
            description: "Index source files into the code knowledge graph. Call this before using get_architectural_context or impact_map to ensure the target files are indexed. Pass no arguments to scan the entire project.".into(),
            prompt: "Use to index files before querying the code graph.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "directory": { "type": "string", "description": "Directory to scan. Omit to scan the entire workspace." }
                }
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

// ── Impact map tool (blast radius, per-call connection) ─────────

struct ImpactMapTool {
    db_path: std::path::PathBuf,
}

#[async_trait]
impl Tool for ImpactMapTool {
    async fn execute(self: Arc<Self>, tool_use: &ToolUse, _ctx: &ToolContext) -> AgentResult<ToolResultMessage> {
        let symbol = tool_use.input.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        if symbol.is_empty() {
            return Err(aegis_core::AgentError::ToolValidationError {
                tool: "impact_map".into(), errors: "symbol is required".into(),
            });
        }
        let start = std::time::Instant::now();
        match <aegis_code_graph::SqliteGraphStore as aegis_code_graph::GraphStore>::open(&self.db_path) {
            Ok(store) => {
                let text = aegis_code_graph::get_impact_map(&store, symbol)
                    .unwrap_or_else(|e| format!("Error: {e}"));
                Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: false,
                    content: vec![ContentBlock::Text { text }],
                    elapsed_ms: start.elapsed().as_millis() as u64 })
            }
            Err(e) => Ok(ToolResultMessage { tool_use_id: tool_use.id.clone(), is_error: true,
                content: vec![ContentBlock::Text { text: format!("DB error: {e}") }],
                elapsed_ms: start.elapsed().as_millis() as u64 }),
        }
    }
}

impl ToolMetadata for ImpactMapTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "impact_map".into(),
            description: "Show what breaks if you change a symbol — blast radius analysis. Returns all dependents (callers, importers) of a function/class/struct.".into(),
            prompt: "Use BEFORE modifying a symbol to understand what else depends on it.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "symbol": { "type": "string", "description": "Symbol name to check (function, struct, class, etc.)" } },
                "required": ["symbol"]
            }),
        }
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low }
    fn concurrency_safety(&self) -> ConcurrencySafety { ConcurrencySafety::ConcurrentSafe }
}

// ── Agent setup ─────────────────────────────────────────────────

fn read_api_key() -> Option<String> {
    let config = cli::load_config();
    if !config.api_key.is_empty() { return Some(config.api_key); }
    std::env::var("DEEPSEEK_API_KEY").ok()
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|s| !s.is_empty())
}

fn spawn_agent(
    shared_mode: Arc<std::sync::RwLock<aegis_core::types::tool::ExecutionMode>>,
    shared_config: Arc<std::sync::RwLock<AgentConfig>>,
) -> anyhow::Result<(mpsc::UnboundedSender<String>, mpsc::UnboundedReceiver<StreamEvent>, Arc<aegis_core::skills::SkillRegistry>)> {
    let api_key = read_api_key()
        .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set. Export it in your environment."))?;

    let config = shared_config.read().unwrap().clone();
    let model = config.default_model.clone();
    let llm = Arc::new(DeepSeekClient::new(api_key, &model)?);
    let registry = Arc::new(ToolRegistry::new());
    let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

    use aegis_tools::*;
    // P0 — file ops (with read-before-edit tracking)
    let read_tracker = Arc::new(aegis_tools::shared::ReadTracker::new());
    registry.register(Arc::new(BashTool::new()))?;
    registry.register(Arc::new(FileReadTool::new().with_read_tracker(read_tracker.clone())))?;
    registry.register(Arc::new(FileEditTool::new().with_read_tracker(read_tracker.clone())))?;
    registry.register(Arc::new(FileWriteTool::new()))?;
    registry.register(Arc::new(ListDirTool))?;
    // P0 — search
    registry.register(Arc::new(GlobTool::new()))?;
    registry.register(Arc::new(GrepTool::new()))?;
    registry.register(Arc::new(FileSearchTool))?;
    // P0 — task mgmt
    registry.register(Arc::new(PlanTool))?;
    registry.register(Arc::new(TodoWriteTool::new()))?;
    registry.register(Arc::new(AskUserTool))?;
    // P1 — review suite
    registry.register(Arc::new(DiagnosticsTool))?;
    registry.register(Arc::new(GitStatusTool))?;
    registry.register(Arc::new(GitDiffTool))?;
    registry.register(Arc::new(GitLogTool))?;
    registry.register(Arc::new(RunTestsTool))?;
    // P2 — advanced
    registry.register(Arc::new(ValidateTool))?;
    registry.register(Arc::new(ApplyPatchTool))?;
    registry.register(Arc::new(RememberTool::new()))?;
    registry.register(Arc::new(ReviewTool))?;
    registry.register(Arc::new(WebFetchTool::new()))?;
    // P2 — new tools
    registry.register(Arc::new(WebSearchTool::new()))?;
    // SkillTool registered later with backend after skill_registry is built
    registry.register(Arc::new(LspTool::new(std::path::PathBuf::from(&config.workspace_dir))))?;
    // Task management (share store with todo_write — unified task list)
    let task_store = aegis_tools::task::TaskStore::default();
    registry.register(Arc::new(TaskCreateTool::new(task_store.clone())))?;
    registry.register(Arc::new(TaskGetTool::new(task_store.clone())))?;
    registry.register(Arc::new(TaskListTool::new(task_store.clone())))?;
    registry.register(Arc::new(TaskUpdateTool::new(task_store.clone())))?;
    registry.register(Arc::new(TaskOutputTool::new()))?;
    registry.register(Arc::new(TaskStopTool::new()))?;
    // Worktree, cron, config
    registry.register(Arc::new(EnterWorktreeTool::new()))?;
    registry.register(Arc::new(ExitWorktreeTool::new()))?;
    let cron_store = CronStore::new();
    registry.register(Arc::new(CronCreateTool::new(cron_store.clone())))?;
    registry.register(Arc::new(CronDeleteTool::new(cron_store.clone())))?;
    registry.register(Arc::new(CronListTool::new(cron_store)))?;
    registry.register(Arc::new(ToolSearchTool::new()))?;
    registry.register(Arc::new(SleepTool::new()))?;
    registry.register(Arc::new(ConfigTool::new()))?;

    // AgentLoop will be created after ALL tool registrations complete
    // (including skill/agent/mcp tools registered below)

    // ── Skill system ──
    let mut skill_registry = aegis_core::skills::SkillRegistry::new();
    skill_registry.register_bundled(
        "code-review", "Review code for bugs and improvements",
        "## Code Review\nWhen reviewing code:\n1. Check for correctness\n2. Check for edge cases\n3. Check for security\n4. Check for performance\n5. Suggest improvements",
        Some("When user asks for code review"),
    );
    skill_registry.register_bundled(
        "debugging", "Systematic debugging workflow",
        "## Debugging\n1. Reproduce the bug\n2. Isolate to minimal case\n3. Hypothesize root cause\n4. Test hypothesis\n5. Fix root cause\n6. Add regression test",
        Some("When user reports a bug or error"),
    );
    skill_registry.register_bundled(
        "rust-best-practices", "Rust coding standards",
        "## Rust Best Practices\n- Prefer &str over String for parameters\n- Use Result<T, E> not panic!\n- Derive Debug for all public types\n- Keep functions small (<50 lines)\n- Use cargo clippy and cargo fmt",
        Some("When writing or reviewing Rust code"),
    );
    let _ = skill_registry.load_project_skills(".");
    let skill_text = skill_registry.injection_text().to_string();
    let skill_arc = Arc::new(skill_registry);

    // Wire SkillTool with actual skill registry
    {
        let sreg = Arc::clone(&skill_arc);
        let skill_tool = Arc::new(aegis_tools::SkillTool::new().with_backend(
            Arc::new(move |name: &str, _args: &str| -> String {
                if let Some(skill) = sreg.get(name) {
                    format!("## Skill Loaded: {}\n\n{}\n\nFollow the instructions above.", skill.name, skill.content)
                } else {
                    format!("Skill '{}' not found. Available: {}",
                        name, sreg.list().iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", "))
                }
            })
        ));
        registry.register(skill_tool)?;
    }

    // Clone before AgentLoop::new (sub-agent runner needs them)
    let subagent_llm = Arc::clone(&llm);
    let subagent_sp = Arc::clone(&sp);

    // ── AgentTool (sub-agent spawning) ──
    {
        let agent_llm = Arc::clone(&subagent_llm);
        let agent_registry = Arc::clone(&registry);
        let agent_sp = Arc::clone(&subagent_sp);
        let agent_config = config.clone();
        let agent_customs = aegis_core::agent::load_agents_dir(&std::path::PathBuf::from(&config.workspace_dir));

        let runner: aegis_tools::agent::SubagentRunner = Arc::new(move |def: aegis_core::agent::AgentDefinition, prompt: String| {
            let agent_llm = Arc::clone(&agent_llm);
            let agent_registry = Arc::clone(&agent_registry);
            let agent_sp = Arc::clone(&agent_sp);
            let agent_config = agent_config.clone();
            Box::pin(async move {
                let sub_config = {
                    let mut c = agent_config.clone();
                    if let Some(ref m) = def.model { c.default_model = m.clone(); }
                    if let Some(t) = def.max_turns { c.max_turns = t; }
                    c.verify_before_output = false;
                    c
                };

                let sub_registry = Arc::new(aegis_core::tool_system::registry::ToolRegistry::new());
                let parent_names: Vec<String> = agent_registry.tool_names();
                let allow = def.tools.as_ref();
                let disallow: std::collections::HashSet<&str> = def.disallowed_tools.iter().map(|s| s.as_str()).collect();
                for name in &parent_names {
                    let skip = if let Some(a) = allow { !a.contains(name) } else { false };
                    if skip || disallow.contains(name.as_str()) { continue; }
                    if let Some(tool) = agent_registry.get_clone(name) {
                        let _ = sub_registry.register(tool);
                    }
                }

                let sub_sp = Arc::new(aegis_core::agent::system_prompt::SystemPromptBuilder::new(sub_config.clone()));
                if let Ok(frozen) = agent_sp.get_frozen_prefix() {
                    sub_sp.freeze_tools(&frozen);
                }

                let start = std::time::Instant::now();
                let mut sub_agent = aegis_core::agent::AgentLoop::<aegis_core::llm::deepseek::DeepSeekClient>::new(
                    sub_config, Arc::clone(&agent_llm), sub_registry, sub_sp,
                );
                sub_agent.set_skills_injection(def.system_prompt.clone());
                let result = sub_agent.run(&prompt).await;
                let elapsed = start.elapsed().as_millis() as u64;

            match result {
                Ok(output) => aegis_core::agent::SubagentResult {
                    agent_name: def.name.clone(),
                    output: output.content,
                    tokens_used: 0, // TokenUsage doesn't expose total
                    elapsed_ms: elapsed,
                    error: None,
                    model: def.model.clone().unwrap_or_else(|| "inherit".into()),
                },
                Err(e) => aegis_core::agent::SubagentResult {
                    agent_name: def.name.clone(), output: String::new(),
                    tokens_used: 0, elapsed_ms: elapsed,
                    error: Some(e.to_string()),
                    model: def.model.clone().unwrap_or_else(|| "inherit".into()),
                },
            }
            })
        });

        registry.register(Arc::new(aegis_tools::AgentTool::new()
            .with_customs(agent_customs)
            .with_runner(runner)))?;
    }

    // ── MCP system ──
    let mcp_manager = Arc::new(aegis_mcp::McpConnectionManager::new());
    // Load .mcp.json config if it exists
    let mcp_config = aegis_mcp::load_mcp_config(&std::path::PathBuf::from(&config.workspace_dir))
        .unwrap_or_default();
    if !mcp_config.mcp_servers.is_empty() {
        mcp_manager.configure(mcp_config.mcp_servers);
        // Connect to MCP servers in background
        let mgr_clone = Arc::clone(&mcp_manager);
        tokio::spawn(async move {
            mgr_clone.connect_all();
        });
    }
    // Register MCP tools
    registry.register(Arc::new(aegis_mcp::McpToolImpl::new(Arc::clone(&mcp_manager))))?;
    registry.register(Arc::new(aegis_mcp::ListMcpResourcesTool::new(Arc::clone(&mcp_manager))))?;
    registry.register(Arc::new(aegis_mcp::ReadMcpResourceTool::new(Arc::clone(&mcp_manager))))?;

    // ── Hooks dispatcher ──
    let hooks = Arc::new(aegis_core::hooks::HookDispatcher::new());
    hooks.dispatch(aegis_core::hooks::HookEvent::SessionStart);

    // ── Sandbox ──
    // ── Freeze + AgentLoop (after ALL tools registered) ──
    let tools_json = registry.get_anthropic_tools_json();
    sp.freeze_tools(&tools_json);
    let mut agent = AgentLoop::new(config.clone(), llm, Arc::clone(&registry), sp);
    agent = agent.with_shared_config(Arc::clone(&shared_config));
    let scorer = Arc::new(aegis_core::llm::scorer::RuleBasedScorer);
    agent = agent.with_code_scorer(scorer);
    if !skill_text.is_empty() {
        agent = agent.with_skills(skill_text);
    }

    if config.sandbox_backend != "none" {
        use aegis_core::types::sandbox::SandboxBackend;
        let perms = match config.sandbox_mode.as_str() {
            "full" => aegis_core::types::sandbox::SandboxPermissions::full_access(),
            "workspace-write" => aegis_core::types::sandbox::SandboxPermissions::read_write_workspace("."),
            _ => aegis_core::types::sandbox::SandboxPermissions::read_only_workspace("."),
        };
        let backend = aegis_sandbox::ProcessBackend;
        match backend.spawn(perms) {
            Ok(instance) => {
                let instance: Arc<std::sync::Mutex<Box<dyn aegis_core::types::sandbox::SandboxInstance>>> =
                    Arc::new(std::sync::Mutex::new(instance));
                agent = agent.with_sandbox(instance);
                tracing::info!("Sandbox enabled: process backend");
            }
            Err(e) => {
                tracing::warn!("Sandbox unavailable: {e}. Running without sandbox.");
            }
        }
    }

    // Wire causal memory (full: read + write + consolidation)
    let memory_db = std::path::PathBuf::from(".agent/memory.db");
    let memory_store: Option<Arc<dyn aegis_memory::MemoryStore>> =
        match <aegis_memory::SqliteMemoryStore as aegis_memory::MemoryStore>::open(&memory_db) {
            Ok(store) => Some(Arc::new(store)),
            Err(e) => { tracing::warn!("Memory store unavailable: {e}"); None },
        };
    let episode_mgr: Option<Arc<aegis_memory::EpisodeManager>> = memory_store.as_ref().map(|store| {
        let gater = Arc::new(aegis_memory::CraniMemGater::new(0.3));
        Arc::new(aegis_memory::EpisodeManager::new(Arc::clone(store), gater))
    });
    let consolidator: Option<Arc<aegis_memory::DreamConsolidator>> = memory_store.as_ref().map(|store| {
        Arc::new(aegis_memory::DreamConsolidator::new(
            Arc::clone(store),
            aegis_memory::ConsolidationConfig::default(),
        ))
    });
    let memory_db2 = memory_db.clone();
    // Try to init embedder (best-effort; requires embedding feature + MSVC on Windows)
    #[cfg(feature = "embedding")]
    let embedder: Option<Arc<aegis_memory::Embedder>> = match aegis_memory::Embedder::new() {
        Ok(e) => { tracing::info!("Semantic memory embedder initialized"); Some(Arc::new(e)) }
        Err(e) => { tracing::warn!("Embedder unavailable ({e}), using string search"); None }
    };
    #[cfg(not(feature = "embedding"))]
    let embedder: Option<Arc<()>> = None;

    let _embedder2 = embedder.clone();

    agent = agent.with_memory(Arc::new(move |query: &str| -> String {
        if !memory_db2.exists() { return String::new(); }
        match <aegis_memory::SqliteMemoryStore as aegis_memory::MemoryStore>::open(&memory_db2) {
            Ok(store) => {
                let mut results = Vec::new();

                // Semantic search (only when embedding feature is enabled)
                #[cfg(feature = "embedding")]
                if let Some(ref emb) = embedder2 {
                    if let Ok(vec) = emb.embed_one(query) {
                        if let Ok(all) = store.get_all_embeddings(None) {
                            let knn = aegis_memory::knn_search(&vec, &all, 5);
                            for (id, _score) in &knn {
                                if let Ok(Some(node)) = store.get_node(id) {
                                    results.push(format!("[semantic] {}", node.content_summary()));
                                }
                            }
                        }
                    }
                }

                // String-based search (always available)
                if let Ok(bugs) = store.find_bugs_by_signature(query) {
                    for b in bugs.iter().take(3) {
                        results.push(format!("  [Bug] {} (occurrences: {}) — {}", b.error_message, b.occurrence_count, b.description));
                    }
                }
                // Note: Preference search requires dedicated API; will be added when store supports it
                if results.is_empty() {
                    String::new()
                } else {
                    format!("=== Relevant Memories ===\n{}", results.join("\n"))
                }
            }
            Err(_) => String::new(),
        }
    }));

    // Wire code graph (callback + callable tool + sync scan on startup)
    let graph_db = std::path::PathBuf::from(".agent/code_graph.db");
    let graph_db2 = graph_db.clone();
    agent = agent.with_graph(Arc::new(move |query: &str| -> String {
        if !graph_db.exists() { return String::new(); }
        match <aegis_code_graph::SqliteGraphStore as aegis_code_graph::GraphStore>::open(&graph_db) {
            Ok(store) => {
                aegis_code_graph::get_architectural_context(&store, query).unwrap_or_default()
            }
            Err(_) => String::new(),
        }
    }));

    // Code graph: register tools immediately, scan in background
    {
        let db_path = graph_db2.clone();
        let db_path2 = db_path.clone();
        let scan_db = graph_db2.clone();
        registry.register(Arc::new(LazyCodeGraphTool { db_path }))?;
        registry.register(Arc::new(ImpactMapTool { db_path: db_path2 }))?;
        registry.register(Arc::new(ScanCodebaseTool { db_path: scan_db }))?;
    }

    // ── Codebase overview loaded in background after TUI starts ──

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
    let (stream_tx, stream_rx) = mpsc::unbounded_channel::<StreamEvent>();

    // Wire ask_user callback — sends event + blocks for UI response
    let ask_stream = stream_tx.clone();
    let sm_for_cb = Arc::clone(&shared_mode);
    agent = agent.with_ask_user(Arc::new(move |question_json: &str, header: &str| -> String {
        let parsed: serde_json::Value = serde_json::from_str(question_json).unwrap_or_default();
        let first_q = parsed.get("questions").and_then(|qs| qs.as_array()).and_then(|a| a.first());
        let question_text = first_q.and_then(|q| q.get("question").and_then(|v| v.as_str())).unwrap_or("Question").to_string();
        let options: Vec<aegis_core::llm::client::AskOption> = first_q
            .and_then(|q| q.get("options").and_then(|o| o.as_array().cloned()))
            .map(|arr| arr.iter().filter_map(|opt| {
                Some(aegis_core::llm::client::AskOption {
                    label: opt.get("label")?.as_str()?.to_string(),
                    description: opt.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                })
            }).collect())
            .unwrap_or_default();

        // Detect plan approval context before moving strings
        let is_plan_approval = header.contains("Plan") || header.contains("plan")
            || question_text.contains("approve") || question_text.contains("Approved");

        let _ = ask_stream.send(StreamEvent::AskUser {
            question: question_text,
            header: header.to_string(),
            options,
        });

        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(0);
        *ASK_RESPONSE.lock().unwrap() = Some(tx);

        let answer = rx.recv().unwrap_or_default();

        // On plan approval, switch to Yolo for execution
        if is_plan_approval && !answer.is_empty() && !answer.contains("cancel") && !answer.contains("Cancel") {
            *sm_for_cb.write().unwrap() = aegis_core::types::tool::ExecutionMode::Yolo;
            tracing::info!("Plan approved — switching to Yolo mode for execution");
        }

        answer
    }));

    let episode_mgr2 = episode_mgr.clone();
    let consolidator2 = consolidator.clone();
    let mut turn_count = 0u64;

    let mode_ref = Arc::clone(&shared_mode);
    // ── Snapshots ──
    let snapshot_mgr = Arc::new(std::sync::Mutex::new(
        aegis_core::snapshots::SnapshotManager::new(aegis_core::snapshots::SnapshotConfig::default())
    ));
    let snapshot_mgr2 = Arc::clone(&snapshot_mgr);

    tokio::spawn(async move {
        while let Some(text) = input_rx.recv().await {
            let tx = stream_tx.clone();
            turn_count += 1;

            // Sync mode from TUI
            let current_mode = *mode_ref.read().unwrap();
            agent.set_mode(current_mode);

            // Handle session resume command
            if text.starts_with("__RESUME__") {
                let path = text.trim_start_matches("__RESUME__").trim();
                // Load conversation history from session file
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                        if let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) {
                            // Rebuild conversation state from saved messages
                            let mut healed: Vec<aegis_core::types::message::Message> = msgs.iter().filter_map(|m| {
                                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
                                let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                match role {
                                    "user" => Some(aegis_core::types::message::Message::User(
                                        aegis_core::types::message::UserMessage {
                                            id: format!("resume_{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
                                            timestamp: chrono::Utc::now(),
                                            content: content.to_string(),
                                            metadata: Default::default(),
                                        })),
                                    "assistant" => Some(aegis_core::types::message::Message::Assistant(
                                        aegis_core::types::message::AssistantMessage {
                                            id: format!("resume_{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
                                            timestamp: chrono::Utc::now(),
                                            thinking: None,
                                            content: Some(content.to_string()),
                                            tool_uses: vec![],
                                            model: None,
                                            usage: None,
                                            stop_reason: None,
                                        })),
                                    _ => None,
                                }
                            }).collect();
                            // Heal loaded messages before use
                            aegis_core::agent::healing::heal_loaded_messages(&mut healed);
                            // Replay into conversation
                            for msg in healed {
                                agent.conversation_mut().add_message(msg);
                            }
                        }
                    }
                }
                let _ = stream_tx.send(StreamEvent::TextDelta("[Session restored — resuming from where you left off]\n".into()));
                let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                    content: None, reasoning: None, tool_uses: vec![],
                    stop_reason: Some("__turn_complete__".into()),
                    usage: Default::default(), model: String::new(), latency_ms: 0,
                }));
                continue;
            }

            // Handle mid-turn steer command
            if text.starts_with("__STEER__") {
                let steer_text = text.trim_start_matches("__STEER__").trim();
                if !steer_text.is_empty() {
                    agent.steer(steer_text);
                }
                let _ = stream_tx.send(StreamEvent::TextDelta(format!("\n⤷ {}\n", steer_text)));
                // Don't emit Done — keep the current turn running
                continue;
            }

            // Handle compact command
            if text == "__COMPACT__" {
                let msg = agent.compact_now();
                let _ = stream_tx.send(StreamEvent::TextDelta(format!("\n[{}]", msg)));
                let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                    content: None, reasoning: None, tool_uses: vec![],
                    stop_reason: Some("__turn_complete__".into()),
                    usage: Default::default(), model: String::new(), latency_ms: 0,
                }));
                continue;
            }

            // Handle codebase overview injection (loaded in background after startup)
            if text.starts_with("__OVERVIEW__") {
                let overview = text.trim_start_matches("__OVERVIEW__").trim();
                if !overview.is_empty() {
                    agent.set_codebase_overview(overview.to_string());
                    tracing::info!("Codebase overview injected (background load complete)");
                }
                continue;
            }

            // Handle skill injection command
            if text.starts_with("__SKILL__") {
                let skill_prompt = text.trim_start_matches("__SKILL__").trim();
                if !skill_prompt.is_empty() {
                    agent.append_skill_injection(skill_prompt);
                }
                let _ = stream_tx.send(StreamEvent::TextDelta("\n[Skill prompt injected — run your next message to use it]\n".into()));
                let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                    content: None, reasoning: None, tool_uses: vec![],
                    stop_reason: Some("__turn_complete__".into()),
                    usage: Default::default(), model: String::new(), latency_ms: 0,
                }));
                continue;
            }

            // Handle goal command
            if text.starts_with("__GOAL__") {
                let goal_text = text.trim_start_matches("__GOAL__");
                let parts: Vec<&str> = goal_text.splitn(2, "\nCriteria: ").collect();
                let objective = parts.first().unwrap_or(&"").trim();
                let criteria: Vec<String> = if parts.len() > 1 {
                    parts[1].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                } else { vec![] };
                let mut contract = aegis_core::agent::SprintContract::new(objective.to_string());
                for c in &criteria {
                    contract.acceptance_criteria.push(aegis_core::agent::AcceptanceCriterion {
                        description: c.clone(), verification_command: String::new(),
                        expected_exit_code: 0, expected_output_contains: None,
                    });
                }
                agent.set_goal(contract);
                let criteria_str = if criteria.is_empty() { "auto-judge".to_string() } else { criteria.join(", ") };
                let real_prompt = format!("Goal: {}\nCriteria: {}\nWork towards this goal. The system will automatically verify when it's complete.", objective, criteria_str);
                let result = agent.run_streaming(&real_prompt, &move |event: StreamEvent| { let _ = tx.send(event); }).await;
                let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                    content: None, reasoning: None, tool_uses: vec![],
                    stop_reason: Some("__turn_complete__".into()),
                    usage: Default::default(), model: String::new(), latency_ms: 0,
                }));
                match result {
                    Ok(_output) => { tracing::info!("Goal turn complete"); }
                    Err(e) => { let _ = stream_tx.send(StreamEvent::TextDelta(format!("\n[Error] {e}\n"))); }
                }
                continue;
            }

            // Episode: open new memory episode
            let episode_id = episode_mgr2.as_ref()
                .map(|mgr| mgr.open("default", &text).ok())
                .flatten();

            // Pre-turn snapshot
            let snap_label = format!("turn-{}", turn_count);
            let _ = snapshot_mgr2.lock().unwrap().snapshot_pre_turn(&snap_label);

            let result = agent.run_streaming(&text, &move |event: StreamEvent| { let _ = tx.send(event); }).await;

            // Post-turn snapshot
            let _ = snapshot_mgr2.lock().unwrap().snapshot_post_turn(&snap_label);
            // Always emit Done at end of turn (belt-and-suspenders for exit paths)
            let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                content: None, reasoning: None, tool_uses: vec![],
                stop_reason: Some("__turn_complete__".into()),
                usage: Default::default(), model: String::new(), latency_ms: 0,
            }));

            // Episode: close with outcome
            if let (Some(mgr), Some(ep_id)) = (episode_mgr2.as_ref(), &episode_id) {
                let outcome = match &result {
                    Ok(_) => aegis_memory::EpisodeOutcome::Success,
                    Err(_) => aegis_memory::EpisodeOutcome::Failure,
                };
                let response_text = result.as_ref().map(|o| o.content.as_str()).unwrap_or("");
                let error_sig = result.as_ref().err().map(|e| aegis_memory::compute_error_signature(&e.to_string()));
                let correction = if aegis_memory::is_user_correction(&text) { Some(text.as_str()) } else { None };
                let _ = mgr.close(&ep_id, outcome, response_text, error_sig.as_deref(), correction);
            }

            // Auto-save session after each turn (with message history for /resume)
            let _ = std::fs::create_dir_all(".agent/sessions");
            let session_file = format!(".agent/sessions/session-{:03}.json", turn_count);
            let conv_msgs: Vec<serde_json::Value> = agent.conversation().messages().iter()
                .filter_map(|m| match m {
                    aegis_core::types::message::Message::User(u) => Some(serde_json::json!({"role":"user","content":u.content})),
                    aegis_core::types::message::Message::Assistant(a) => Some(serde_json::json!({"role":"assistant","content":a.content.as_deref().unwrap_or("")})),
                    _ => None,
                }).collect();
            let session_data = serde_json::json!({
                "turn": turn_count,
                "user_input": text,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "messages": conv_msgs,
            });
            let _ = std::fs::write(&session_file, serde_json::to_string_pretty(&session_data).unwrap_or_default());

            match result {
                Ok(output) => {
                    tracing::info!(confidence = ?output.confidence, content_len = output.content.len(), "turn complete");
                }
                Err(e) => {
                    let _ = stream_tx.send(StreamEvent::TextDelta(format!("\n[Error] {e}\n")));
                }
            }

            // Consolidation: check every 5 turns
            if turn_count % 5 == 0 {
                if let Some(ref consolidator) = consolidator2 {
                    if consolidator.should_consolidate().unwrap_or(false) {
                        let c = Arc::clone(consolidator);
                        tokio::task::spawn_blocking(move || {
                            match c.consolidate() {
                                Ok(r) => tracing::info!(insights = r.insights_generated, pruned = r.pruned_count, "consolidation complete"),
                                Err(e) => tracing::warn!("consolidation failed: {e}"),
                            }
                        });
                    }
                }
            }
        }
    });

    Ok((input_tx, stream_rx, skill_arc))
}

// ── Main ────────────────────────────────────────────────────────

mod cli;
mod tui;
use tui::render::*;
pub use tui::types::*;

fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let args = cli::Cli::parse();

    // Subcommand: config
    if let Some(cli::Command::Config) = args.command {
        let dir = cli::ensure_config_dir();
        println!("Aegis config directory: {}", dir.display());
        let cfg_path = dir.join("config.toml");
        if cfg_path.exists() {
            println!("Config file: {}", cfg_path.display());
        } else {
            println!("No config.toml found. Create one at {} with:", cfg_path.display());
            println!("  [api]");
            println!("  api_key = \"sk-...\"");
            println!("  model = \"deepseek-v4-pro\"");
        }
        return Ok(());
    }

    // ── Chat mode (default) ──
    install_panic_hook();

    let config = cli::load_config();
    let model = if !args.model.is_empty() && args.model != "deepseek-v4-pro" { args.model.clone() } else { config.model.clone() };
    let effort = if !args.effort.is_empty() && args.effort != "max" { args.effort.clone() } else { config.effort.clone() };

    let rt = tokio::runtime::Runtime::new()?;
    let _rt_guard = rt.enter();

    // Shared mode for TUI↔Agent sync
    let shared_mode: Arc<std::sync::RwLock<aegis_core::types::tool::ExecutionMode>> =
        Arc::new(std::sync::RwLock::new(aegis_core::types::tool::ExecutionMode::Default));
    let shared_config: Arc<std::sync::RwLock<AgentConfig>> =
        Arc::new(std::sync::RwLock::new(AgentConfig::default()));

    let (agent_tx, stream_rx, skill_registry) = match spawn_agent(Arc::clone(&shared_mode), Arc::clone(&shared_config)) {
        Ok(triple) => triple,
        Err(e) => {
            // Interactive first-run setup — no key configured yet
            println!("\n  {}", e);
            println!("\n  Aegis 需要 DeepSeek API Key 才能运行。");
            println!("  获取: https://platform.deepseek.com/api_keys\n");
            print!("  请输入 API Key: ");
            let _ = io::stdout().flush();
            let mut key = String::new();
            io::stdin().read_line(&mut key).ok();
            let key = key.trim().to_string();
            if key.is_empty() { anyhow::bail!("未输入 API Key，已取消。"); }

            // Save to config file
            let dir = cli::ensure_config_dir();
            let path = dir.join("config.toml");
            let config_toml = format!(
                "api_key = \"{}\"\nmodel = \"deepseek-v4-pro\"\neffort = \"max\"\n",
                key.trim()
            );
            std::fs::write(&path, &config_toml)?;
            println!("  已保存到 {}\n", path.display());

            // Set env var for current session and retry
            unsafe { std::env::set_var("DEEPSEEK_API_KEY", &key); }
            match spawn_agent(Arc::clone(&shared_mode), Arc::clone(&shared_config)) {
                Ok(triple) => triple,
                Err(e2) => anyhow::bail!("{e2}"),
            }
        }
    };

    // ── Load codebase overview in background (don't block TUI startup) ──
    let overview_tx = agent_tx.clone();
    let overview_db_path = std::path::PathBuf::from(".agent/code_graph.db");
    tokio::task::spawn_blocking(move || {
        if !overview_db_path.exists() { return; }
        match aegis_code_graph::SqliteGraphStore::open(&overview_db_path) {
            Ok(store) => {
                match aegis_code_graph::get_codebase_overview(&store) {
                    Ok(overview) if !overview.is_empty() => {
                        let _ = overview_tx.send(format!("__OVERVIEW__{}", overview));
                    }
                    _ => {}
                }
            }
            Err(_) => {}
        }
    });

    let _guard = TermGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut term = Terminal::new(backend)?;

    // ── ACP Server (HTTP/SSE) ──
    let (acp_tx, _) = tokio::sync::broadcast::channel::<aegis_mcp::acp::AcpStreamEvent>(256);
    let acp_state = Arc::new(aegis_mcp::acp::AcpState::new(
        agent_tx.clone(),
        acp_tx.clone(),
    ));
    let acp_router = aegis_mcp::acp::acp_router(Arc::clone(&acp_state));
    let acp_port = config.acp_port;
    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{}", acp_port);
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        tracing::info!("ACP server listening on http://{}", addr);
        axum::serve(listener, acp_router).await.ok();
    });

    let mut app = App {
        input: String::with_capacity(256),
        cursor_byte: 0,
        input_scroll_x: 0,
        messages: Vec::with_capacity(200),
        scroll: 0,
        viewport_height: 24,
        agent_tx,
        stream_rx,
        quit: false,
        model,
        mode: "default".to_string(),
        reasoning_effort: effort,
        tokens_in: 0,
        tokens_out: 0,
        cache_tokens: 0,
        cost: 0.0,
        running: false,
        turn_start: None,
        last_turn_ms: 0,
        turn_tokens_in: 0,
        turn_tokens_out: 0,
        turn_tokens_cache: 0, last_call_cache_pct: 0.0,
        last_assist_idx: None,
        sel_start: None,
        sel_end: None,
        msg_area_h: 24,
        msg_area_y: 0,
        lines_buf: Vec::new(),
        input_y: 0,
        input_h: 5,
        dialog: None,
        model_dialog: None,
        skill_dialog: None,
        session_dialog: None,
        skill_registry: Some(skill_registry),
        sandbox_enabled: true,
        paste_buf: Vec::new(),
        paste_counter: 0,
        shared_mode: Some(Arc::clone(&shared_mode)),
        shared_config: Some(Arc::clone(&shared_config)),
    };

    let mut throttle = Throttle::new();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown);
    rt.spawn(async move { let _ = tokio::signal::ctrl_c().await; shutdown_flag.store(true, Ordering::SeqCst); });

    let acp_tx_ref = acp_tx.clone();
    while !app.quit && !shutdown.load(Ordering::SeqCst) {
        if crossterm::event::poll(Duration::from_millis(10)).unwrap_or(false) {
            if let Ok(event) = crossterm::event::read() {
                match event {
                    CEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

                        // Session picker dialog
                        if let Some(ref mut sd) = app.session_dialog {
                            match (k.code, ctrl) {
                                (KeyCode::Esc, false) => { app.session_dialog = None; }
                                (KeyCode::Up, false) => { if sd.session_idx > 0 { sd.session_idx -= 1; } }
                                (KeyCode::Down, false) => { if sd.session_idx + 1 < sd.sessions.len() { sd.session_idx += 1; } }
                                (KeyCode::Enter, _) => {
                                    let (filename, _, _, _) = &sd.sessions[sd.session_idx];
                                    let marker = format!("/resume {}", filename.trim_end_matches(".json"));
                                    app.input = marker;
                                    app.cursor_byte = app.input.len();
                                    app.session_dialog = None;
                                }
                                _ => {}
                            }
                            throttle.force = true;
                            continue;
                        }

                        // Skill picker dialog
                        if let Some(ref mut sd) = app.skill_dialog {
                            match (k.code, ctrl) {
                                (KeyCode::Esc, false) => { app.skill_dialog = None; }
                                (KeyCode::Up, false) => { if sd.skill_idx > 0 { sd.skill_idx -= 1; } }
                                (KeyCode::Down, false) => { if sd.skill_idx + 1 < sd.skills.len() { sd.skill_idx += 1; } }
                                (KeyCode::Enter, _) => {
                                    let (name, _) = &sd.skills[sd.skill_idx];
                                    let marker = format!("/skill {}", name);
                                    app.input = marker;
                                    app.cursor_byte = app.input.len();
                                    app.skill_dialog = None;
                                }
                                _ => {}
                            }
                            throttle.force = true;
                            continue;
                        }

                        // Model picker dialog
                        if let Some(ref mut md) = app.model_dialog {
                            match (k.code, ctrl) {
                                (KeyCode::Esc, false) => { app.model_dialog = None; }
                                (KeyCode::Up, false) => { if md.model_idx > 0 { md.model_idx -= 1; } }
                                (KeyCode::Down, false) => { if md.model_idx + 1 < md.models.len() { md.model_idx += 1; } }
                                (KeyCode::Left, false) => { if md.effort_idx > 0 { md.effort_idx -= 1; } }
                                (KeyCode::Right, false) => { if md.effort_idx + 1 < EFFORTS.len() { md.effort_idx += 1; } }
                                (KeyCode::Enter, _) => {
                                    let (model_id, _) = md.models[md.model_idx];
                                    app.model = model_id.to_string();
                                    if let Some(ref cfg) = app.shared_config {
                                        cfg.write().unwrap().reasoning_effort = EFFORTS[md.effort_idx].to_string();
                                    }
                                    app.messages.push(Msg::System(format!("Model: {}  Thinking: {}", model_id, EFFORTS[md.effort_idx])));
                                    app.model_dialog = None;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Ask dialog mode
                        if let Some(ref mut dlg) = app.dialog {
                            let total = dlg.options.len() + 2;
                            match (k.code, ctrl) {
                                (KeyCode::Esc, false) => {
                                    if let Some(tx) = ASK_RESPONSE.lock().unwrap().take() {
                                        let _ = tx.send(String::new());
                                    }
                                    app.dialog = None;
                                }
                                (KeyCode::Up, false) => {
                                    if dlg.selected > 0 { dlg.selected -= 1; }
                                    dlg.in_custom = false;
                                }
                                (KeyCode::Down, false) => {
                                    if dlg.selected + 1 < total { dlg.selected += 1; }
                                    dlg.in_custom = false;
                                }
                                (KeyCode::Enter, _) => {
                                    if dlg.in_custom {
                                        let answer = std::mem::take(&mut dlg.custom_input);
                                        dlg.custom_cursor = 0;
                                        if let Some(tx) = ASK_RESPONSE.lock().unwrap().take() {
                                            let _ = tx.send(answer);
                                        }
                                        app.dialog = None;
                                    } else if dlg.selected < dlg.options.len() {
                                        let answer = dlg.options[dlg.selected].clone();
                                        if let Some(tx) = ASK_RESPONSE.lock().unwrap().take() {
                                            let _ = tx.send(answer);
                                        }
                                        app.dialog = None;
                                    } else if dlg.selected == dlg.options.len() {
                                        dlg.in_custom = true;
                                        dlg.custom_input.clear();
                                        dlg.custom_cursor = 0;
                                    } else {
                                        if let Some(tx) = ASK_RESPONSE.lock().unwrap().take() {
                                            let _ = tx.send(String::new());
                                        }
                                        app.dialog = None;
                                    }
                                }
                                (KeyCode::Backspace, _) if dlg.in_custom => {
                                    if dlg.custom_cursor > 0 {
                                        let mut p = dlg.custom_cursor - 1;
                                        while p > 0 && !dlg.custom_input.is_char_boundary(p) { p -= 1; }
                                        dlg.custom_input.remove(p);
                                        dlg.custom_cursor = p;
                                    }
                                }
                                (KeyCode::Char(ch), false) if dlg.in_custom => {
                                    dlg.custom_input.insert(dlg.custom_cursor, ch);
                                    dlg.custom_cursor += ch.len_utf8();
                                }
                                _ => {}
                            }
                            throttle.force = true;
                            continue;
                        }

                        match (k.code, ctrl) {
                            (KeyCode::Char('c'), true) if app.sel_start.is_some() => {
                                // Copy selection to clipboard (suppress errors — clipboard may be locked)
                                if let (Some(a), Some(b)) = (app.sel_start, app.sel_end) {
                                    let lo = a.min(b); let hi = a.max(b);
                                    if lo <= hi && hi < app.lines_buf.len() {
                                        let text: String = app.lines_buf[lo..=hi].join("\n");
                                        if !text.trim().is_empty() {
                                            match Clipboard::new() {
                                                Ok(mut c) => { let _ = c.set_text(text); }
                                                Err(_) => {} // clipboard unavailable — silently skip
                                            }
                                        }
                                    }
                                }
                                app.sel_start = None; app.sel_end = None;
                            }
                            (KeyCode::Esc, false) => {
                                // Priority 1: if agent is running, cancel the turn
                                if app.running {
                                    app.running = false;
                                    app.messages.push(Msg::System("[Cancelled]".into()));
                                    app.scroll_to_bottom();
                                }
                                // Priority 2: clear text selection
                                else if app.sel_start.is_some() { app.sel_start = None; app.sel_end = None; }
                                // Priority 3: empty input and not running → exit
                                else if app.input.is_empty() { app.quit = true; }
                                // Priority 4: clear input field
                                else { app.input.clear(); app.cursor_byte = 0; app.input_scroll_x = 0; }
                            }
                            (KeyCode::Char('d'), true) if app.input.is_empty() => { app.quit = true; }
                            (KeyCode::Char('c'), true) => { app.input.clear(); app.cursor_byte = 0; app.input_scroll_x = 0; }
                            (KeyCode::Backspace, _) => { app.backspace(); }
                            (KeyCode::Delete, _) => { app.delete_forward(); }
                            (KeyCode::Left, _) => { app.cursor_left(); }
                            (KeyCode::Right, _) => { app.cursor_right(); }
                            (KeyCode::Home, _) => { app.cursor_home(); }
                            (KeyCode::End, _) => { app.cursor_end(); }
                            (KeyCode::Enter, _) if k.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.input.insert(app.cursor_byte, '\n');
                                app.cursor_byte += 1;
                                throttle.force = true;
                            }
                            (KeyCode::Char(ch), false) if ch == '\n' || ch == '\r' => {
                                app.input.insert(app.cursor_byte, '\n');
                                app.cursor_byte += 1;
                                throttle.force = true;
                            }
                            (KeyCode::Enter, _) => {
                                if app.input.trim().is_empty() { continue; }
                                // Mid-turn steer: agent is running — inject as steering instruction
                                if app.running && !app.input.starts_with('/') && !app.input.starts_with('!') {
                                    let text = std::mem::take(&mut app.input);
                                    app.cursor_byte = 0;
                                    let _ = app.agent_tx.send(format!("__STEER__{}", text));
                                    app.messages.push(Msg::System("[Steered] Instruction queued — agent will adapt mid-turn".into()));
                                    app.scroll_to_bottom();
                                    throttle.force = true;
                                    continue;
                                }
                                if app.input.starts_with('/') {
                                    let text = std::mem::take(&mut app.input);
                                    app.cursor_byte = 0;
                                    let resp = app.handle_slash_command(&text);
                                    if let Some(r) = resp { app.messages.push(Msg::System(r)); }
                                } else if app.input.starts_with('!') {
                                    let text = std::mem::take(&mut app.input);
                                    app.cursor_byte = 0;
                                    let out = app.run_bash(&text);
                                    app.messages.push(Msg::User(text));
                                    app.messages.push(Msg::System(out));
                                } else {
                                    app.submit();
                                }
                            }
                            (KeyCode::Tab, false) => {
                                let text = app.input.clone();
                                if let Some(pos) = text.rfind('@') {
                                    let prefix = &text[pos+1..];
                                    let candidates = App::file_completions(prefix);
                                    if candidates.len() == 1 {
                                        let new_text = format!("{}{}", &text[..pos], candidates[0]);
                                        app.input = new_text; app.cursor_byte = pos + candidates[0].len();
                                    } else if candidates.len() > 1 {
                                        app.messages.push(Msg::System(format!(
                                            "Multiple matches:\n{}", candidates.join("\n")
                                        )));
                                    }
                                }
                            }
                            (KeyCode::PageUp, _) => { app.scroll_up(app.viewport_height.saturating_sub(1).max(1)); }
                            (KeyCode::PageDown, _) => { app.scroll_down(app.viewport_height.saturating_sub(1).max(1)); }
                            (KeyCode::BackTab, _) | (KeyCode::Tab, _) if k.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.cycle_mode();
                            }
                            (KeyCode::Char(ch), false) => {
                                if app.running { app.running = false; }
                                app.insert(ch);
                                throttle.force = true;
                            }
                            // Forward all other key events to textarea (handles arrows, backspace, delete, home, end, etc.)
                            _ => {
                            }
                        }
                        throttle.force = true;
                    }
                    CEvent::Resize(_, _) => { term.clear()?; throttle.force = true; }
                    CEvent::Paste(text) => {
                        let line_count = text.lines().count();
                        if line_count > 5 && text.len() > 200 {
                            app.paste_counter += 1;
                            let id = app.paste_counter;
                            let marker = format!("⟨PASTE:{}⟩", id);
                            app.paste_buf.push(text);
                            for ch in marker.chars() { app.insert(ch); };
                        } else {
                            for ch in text.chars() { app.insert(ch); };
                        }
                        throttle.force = true;
                    }
                    CEvent::Mouse(me) => {
                        let my = me.row;
                        let in_msg = my >= app.msg_area_y && my < app.msg_area_y + app.msg_area_h;

                        match me.kind {
                            MouseEventKind::ScrollUp => { app.scroll_up(3); throttle.force = true; }
                            MouseEventKind::ScrollDown => { app.scroll_down(3); throttle.force = true; }
                            MouseEventKind::Down(MouseButton::Left) if in_msg => {
                                let line = app.scroll + (my - app.msg_area_y) as usize;
                                let line = line.min(app.lines_buf.len().saturating_sub(1));
                                app.sel_start = Some(line);
                                app.sel_end = Some(line);
                                throttle.force = true;
                            }
                            MouseEventKind::Drag(MouseButton::Left) => {
                                let line = app.scroll + (my - app.msg_area_y) as usize;
                                let line = line.min(app.lines_buf.len().saturating_sub(1));
                                app.sel_end = Some(line);
                                throttle.force = true;
                            }
                            MouseEventKind::Up(MouseButton::Left) => {
                                // Clear single-line click (no actual drag)
                                if app.sel_start == app.sel_end {
                                    app.sel_start = None; app.sel_end = None;
                                }
                                // Multi-line drag selections stay until Esc or Ctrl+C
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut got_stream = false;
        while let Ok(event) = app.stream_rx.try_recv() {
            // Forward to ACP broadcast
            let acp_evt = match &event {
                StreamEvent::TextDelta(t) => Some(aegis_mcp::acp::AcpStreamEvent::Text { content: t.clone() }),
                StreamEvent::ToolUseStart { name, input, .. } => Some(aegis_mcp::acp::AcpStreamEvent::ToolUse { name: name.clone(), input: input.clone() }),
                StreamEvent::ToolResult { name, output, .. } => Some(aegis_mcp::acp::AcpStreamEvent::ToolResult { name: name.clone(), output: output.clone() }),
                StreamEvent::Done(resp) => Some(aegis_mcp::acp::AcpStreamEvent::Done {
                    model: resp.model.clone(),
                    tokens_used: resp.usage.output_tokens,
                }),
                _ => None,
            };
            if let Some(evt) = acp_evt {
                let _ = acp_tx_ref.send(evt);
            }
            app.handle_stream(event);
            got_stream = true;
        }
        // Sync mode display from shared state (e.g. plan approval → Yolo)
        if let Some(ref sm) = app.shared_mode {
            let current = *sm.read().unwrap();
            let mode_str = match current {
                aegis_core::types::tool::ExecutionMode::Chat => "chat",
                aegis_core::types::tool::ExecutionMode::Plan => "plan",
                aegis_core::types::tool::ExecutionMode::Default => "default",
                aegis_core::types::tool::ExecutionMode::Yolo => "yolo",
            };
            if app.mode != mode_str {
                app.messages.push(Msg::System(format!("Mode: {} → {}", app.mode, mode_str)));
                app.mode = mode_str.to_string();
            }
            // Sync reasoning effort display
            let effort_str = app.shared_config.as_ref()
                .map(|c| c.read().unwrap().reasoning_effort.clone())
                .unwrap_or_else(|| "max".into());
            if app.reasoning_effort != effort_str {
                app.reasoning_effort = effort_str;
            }
        }
        if got_stream { throttle.force = true; }

        if throttle.should() {
            term.draw(|frame| tui::render::render(frame, &mut app))?;
            throttle.mark();
        }
    }

    let _ = stdout().execute(crossterm::cursor::Show);
    drop(_guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_cycle() {
        let mut app = App {
            input: String::new(), cursor_byte: 0, input_scroll_x: 0,
            messages: vec![], scroll: 0, viewport_height: 24,
            agent_tx: mpsc::unbounded_channel().0, stream_rx: mpsc::unbounded_channel().1,
            quit: false, model: "test".into(), mode: "default".into(), reasoning_effort: "max".into(),
            tokens_in: 0, tokens_out: 0, cache_tokens: 0, cost: 0.0,
            running: false, turn_start: None, last_turn_ms: 0,
            turn_tokens_in: 0, turn_tokens_out: 0, turn_tokens_cache: 0, last_call_cache_pct: 0.0,
            last_assist_idx: None,
            sel_start: None, sel_end: None, msg_area_h: 24, msg_area_y: 0,
            lines_buf: vec![], input_y: 0, input_h: 5, dialog: None, model_dialog: None,
            skill_dialog: None, session_dialog: None, skill_registry: None,
            sandbox_enabled: false, paste_buf: vec![], paste_counter: 0, shared_mode: None, shared_config: None,
        };
        assert_eq!(app.mode, "default");
        app.cycle_mode();
        assert_eq!(app.mode, "yolo");
        app.cycle_mode();
        assert_eq!(app.mode, "chat");
        app.cycle_mode();
        assert_eq!(app.mode, "plan");
        app.cycle_mode();
        assert_eq!(app.mode, "default");
    }

    #[test]
    fn test_timer_fields() {
        let app = App {
            input: String::new(), cursor_byte: 0, input_scroll_x: 0,
            messages: vec![], scroll: 0, viewport_height: 24,
            agent_tx: mpsc::unbounded_channel().0, stream_rx: mpsc::unbounded_channel().1,
            quit: false, model: "test".into(), mode: "default".into(), reasoning_effort: "max".into(),
            tokens_in: 0, tokens_out: 0, cache_tokens: 0, cost: 0.0,
            running: false, turn_start: None, last_turn_ms: 0,
            turn_tokens_in: 0, turn_tokens_out: 0, turn_tokens_cache: 0, last_call_cache_pct: 0.0,
            last_assist_idx: None,
            sel_start: None, sel_end: None, msg_area_h: 24, msg_area_y: 0,
            lines_buf: vec![], input_y: 0, input_h: 5, dialog: None, model_dialog: None,
            skill_dialog: None, session_dialog: None, skill_registry: None,
            sandbox_enabled: false, paste_buf: vec![], paste_counter: 0, shared_mode: None, shared_config: None,
        };
        assert!(app.turn_start.is_none());
        assert_eq!(app.last_turn_ms, 0);
    }
}
