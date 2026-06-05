//! Minimal working TUI for aegis.
//!
//! Layout:
//! ┌──────────────────────────────┐
//! │  [model] [ctx:XX%] ¥X.XXXX  │  ← status bar (bottom-2)
//! ├──────────────────────────────┤
//! │  Messages area               │  ← scrollable
//! │  (user / assistant / tools)  │
//! ├──────────────────────────────┤
//! │ > user input here      █     │  ← input line + cursor
//! │ !cmd /cmd @file #mem         │  ← hint line
//! └──────────────────────────────┘

use std::io::{self, Write, stdout};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
                          LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use aegis_core::agent::system_prompt::SystemPromptBuilder;
use aegis_core::agent::AgentLoop;
use aegis_core::llm::client::StreamEvent;
use aegis_core::llm::deepseek::DeepSeekClient;
use aegis_core::tool_system::registry::ToolRegistry;
use aegis_core::types::config::AgentConfig;

// ── Frame rate ──────────────────────────────────────────────────

const FRAME_MS: u64 = 16;

struct Throttle {
    last: Instant,
    force: bool,
}

impl Throttle {
    fn new() -> Self {
        Self { last: Instant::now() - Duration::from_millis(FRAME_MS), force: false }
    }
    fn should(&self) -> bool { self.force || self.last.elapsed().as_millis() as u64 >= FRAME_MS }
    fn mark(&mut self) { self.last = Instant::now(); self.force = false; }
}

// ── Messages ────────────────────────────────────────────────────

#[derive(Clone)]
enum Msg {
    User(String),
    Asst { text: String, think: String },
    Tool { id: String, name: String, done: bool, ok: bool, detail: String },
    #[allow(dead_code)]
    System(String),
}

// ── App state ───────────────────────────────────────────────────

struct App {
    input: String,
    cursor_byte: usize, // byte offset into input (UTF-8 safe)
    input_scroll_x: u16, // horizontal scroll for long input lines
    messages: Vec<Msg>,
    scroll: usize,     // top line offset
    viewport_height: usize, // visible lines in message area
    agent_tx: mpsc::UnboundedSender<String>,
    stream_rx: mpsc::UnboundedReceiver<StreamEvent>,
    quit: bool,
    model: String,
    tokens_in: u64,
    tokens_out: u64,
    cost: f64,
    running: bool,
    last_assist_idx: Option<usize>, // index of Msg that's currently being streamed
}

enum KeyAction {
    None,
    Submit,
    Quit,
}

impl App {
    // ── input handling ──

    fn byte_to_col(&self) -> u16 {
        let prefix = &self.input[..self.cursor_byte.min(self.input.len())];
        unicode_width::UnicodeWidthStr::width(prefix) as u16
    }

    fn insert(&mut self, ch: char) {
        self.input.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor_byte > 0 {
            let prev = self.cursor_byte - 1;
            while !self.input.is_char_boundary(prev) {
                // find prev char boundary
                let mut p = prev;
                while p > 0 && !self.input.is_char_boundary(p) { p -= 1; }
                self.input.drain(p..self.cursor_byte);
                self.cursor_byte = p;
                return;
            }
            self.input.remove(prev);
            self.cursor_byte = prev;
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor_byte < self.input.len() {
            let end = self.cursor_byte + 1;
            let mut e = end;
            while e < self.input.len() && !self.input.is_char_boundary(e) { e += 1; }
            self.input.drain(self.cursor_byte..e);
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_byte > 0 {
            let mut p = self.cursor_byte - 1;
            while p > 0 && !self.input.is_char_boundary(p) { p -= 1; }
            self.cursor_byte = p;
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_byte < self.input.len() {
            let mut p = self.cursor_byte + 1;
            while p < self.input.len() && !self.input.is_char_boundary(p) { p += 1; }
            self.cursor_byte = p;
        }
    }

    fn cursor_home(&mut self) { self.cursor_byte = 0; }
    fn cursor_end(&mut self) { self.cursor_byte = self.input.len(); }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> KeyAction {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        match (code, ctrl) {
            (KeyCode::Char('c'), true) | (KeyCode::Char('d'), true) if self.input.is_empty() => {
                return KeyAction::Quit;
            }
            (KeyCode::Char('c'), true) => {
                // Ctrl+C while typing — clear input
                self.input.clear();
                self.cursor_byte = 0;
            }
            (KeyCode::Enter, _) => {
                if !self.input.trim().is_empty() {
                    return KeyAction::Submit;
                }
            }
            (KeyCode::Char(ch), false) => self.insert(ch),
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, _) => self.delete_forward(),
            (KeyCode::Left, _) => self.cursor_left(),
            (KeyCode::Right, _) => self.cursor_right(),
            (KeyCode::Home, _) => self.cursor_home(),
            (KeyCode::End, _) => self.cursor_end(),
            _ => {}
        }
        KeyAction::None
    }

    fn submit(&mut self) {
        let text = std::mem::take(&mut self.input);
        self.cursor_byte = 0;
        self.input_scroll_x = 0;
        if text.trim().is_empty() { return; }

        self.messages.push(Msg::User(text.clone()));
        self.scroll_to_bottom();

        let _ = self.agent_tx.send(text);
        self.running = true;
    }

    // ── scrolling ──

    fn scroll_to_bottom(&mut self) {
        self.scroll = self.total_lines().saturating_sub(self.viewport_height);
    }

    fn total_lines(&self) -> usize {
        self.messages.iter().map(|m| match m {
            Msg::User(t) => t.lines().count() + 1,
            Msg::Asst { text, think } => {
                let n = text.lines().count();
                if !think.is_empty() { n + think.lines().count() + 2 } else { n + 1 }
            }
            Msg::Tool { detail, .. } => detail.lines().count().max(1),
            Msg::System(t) => t.lines().count() + 1,
        }).sum()
    }

    fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    fn scroll_down(&mut self, n: usize) {
        let max = self.total_lines().saturating_sub(self.viewport_height);
        self.scroll = (self.scroll + n).min(max);
    }

    fn at_bottom(&self) -> bool {
        self.scroll >= self.total_lines().saturating_sub(self.viewport_height)
    }

    // ── stream handling ──

    fn handle_stream(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::TextDelta(delta) => {
                self.append_or_create_assistant(&delta, "");
            }
            StreamEvent::ThinkingDelta(delta) => {
                self.append_or_create_assistant("", &delta);
            }
            StreamEvent::ToolUseStart { id, name, input } => {
                let detail = format!("{} {}", name,
                    serde_json::to_string(&input).unwrap_or_default());
                self.messages.push(Msg::Tool { id, name, done: false, ok: true, detail });
                self.last_assist_idx = None;
            }
            StreamEvent::ToolProgress { .. } => {}
            StreamEvent::Done(resp) => {
                self.running = false;
                self.tokens_in += resp.usage.input_tokens;
                self.tokens_out += resp.usage.output_tokens;
                self.cost += (resp.usage.input_tokens as f64 * 0.14
                    + resp.usage.output_tokens as f64 * 0.28) / 1_000_000.0;
                // mark last tool as done
                if let Some(Msg::Tool { done, .. }) = self.messages.last_mut() {
                    *done = true;
                }
                self.last_assist_idx = None;
            }
            _ => {}
        }
        if self.at_bottom() { self.scroll_to_bottom(); }
    }

    fn append_or_create_assistant(&mut self, text: &str, think: &str) {
        match self.last_assist_idx {
            Some(idx) => {
                let msg = &mut self.messages[idx];
                if let Msg::Asst { text: t, think: th } = msg {
                    t.push_str(text);
                    th.push_str(think);
                }
            }
            None => {
                self.messages.push(Msg::Asst {
                    text: text.to_string(),
                    think: think.to_string(),
                });
                self.last_assist_idx = Some(self.messages.len() - 1);
            }
        }
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

// ── Rendering ───────────────────────────────────────────────────

fn render_msg<'a>(out: &mut Vec<Line<'a>>, msg: &'a Msg, area_w: u16) {
    let w = area_w.saturating_sub(4) as usize; // margin
    match msg {
        Msg::User(text) => {
            out.push(Line::from(Span::styled("▸ You", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
            for line in text.lines() {
                for wrapped in textwrap::wrap(line, w.max(20)) {
                    out.push(Line::from(Span::raw(wrapped.to_string())));
                }
            }
            out.push(Line::from(""));
        }
        Msg::Asst { text, think } => {
            if !think.is_empty() {
                out.push(Line::from(Span::styled("  Thinking…", Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC))));
                for line in think.lines() {
                    for wrapped in textwrap::wrap(line, w.max(20)) {
                        out.push(Line::from(Span::styled(wrapped.to_string(),
                            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))));
                    }
                }
            }
            if !text.is_empty() {
                for line in text.lines() {
                    for wrapped in textwrap::wrap(line, w.max(20)) {
                        out.push(Line::from(Span::raw(wrapped.to_string())));
                    }
                }
            }
        }
        Msg::Tool { name, done, ok, detail, .. } => {
            let icon = if *done { if *ok { "+" } else { "x" } } else { "…" };
            let color = if *done { if *ok { Color::Green } else { Color::Red } } else { Color::Yellow };
            out.push(Line::from(vec![
                Span::styled(format!("  {icon} {name}"), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}", detail), Style::default().fg(Color::Gray)),
            ]));
        }
        Msg::System(text) => {
            for line in text.lines() {
                out.push(Line::from(Span::styled(line, Style::default().fg(Color::Yellow))));
            }
        }
    }
}

fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 20 || area.height < 5 { return; }

    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let [msg_area, status_area, input_area, hint_area] = layout.areas(area);

    // ── messages ──
    app.viewport_height = msg_area.height.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(256);

    if app.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "Welcome to aegis. Type a question and press Enter.",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Ctrl+C    quit (when input empty)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "  Ctrl+C    clear input (when typing)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    for msg in &app.messages {
        render_msg(&mut lines, msg, msg_area.width);
    }

    // If agent is running, show a blinking indicator
    if app.running {
        lines.push(Line::from(Span::styled("|", Style::default().fg(Color::Yellow))));
    }

    // Clip to visible scroll window
    let total = lines.len();
    let start = app.scroll.min(total);
    let end = (start + app.viewport_height).min(total);
    let visible: Vec<Line> = if start < end {
        lines[start..end].to_vec()
    } else {
        vec![]
    };

    // Scroll indicator
    let scroll_text = if total > app.viewport_height && !app.at_bottom() {
        format!(" ↑{} lines above ", app.scroll)
    } else {
        String::new()
    };

    let msg_widget = Paragraph::new(Text::from(visible))
        .block(Block::default().borders(Borders::NONE))
        .scroll((0, 0));
    frame.render_widget(msg_widget, msg_area);

    if !scroll_text.is_empty() {
        let indicator = Paragraph::new(Line::from(
            Span::styled(&scroll_text, Style::default().fg(Color::Yellow).bg(Color::Black))
        )).right_aligned();
        frame.render_widget(indicator, Rect {
            x: msg_area.x,
            y: msg_area.y + msg_area.height.saturating_sub(1),
            width: msg_area.width,
            height: 1,
        });
    }

    // ── status bar ──
    let mut status_parts: Vec<Span> = Vec::new();
    status_parts.push(Span::styled(
        format!("[{}]", app.model),
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
    ));
    if app.tokens_in + app.tokens_out > 0 {
        status_parts.push(Span::raw(format!(
            " tk:{:.0}K/{:.0}K",
            app.tokens_in as f64 / 1000.0,
            app.tokens_out as f64 / 1000.0,
        )));
    }
    status_parts.push(Span::raw(format!(" ¥{:.4}", app.cost)));
    if app.running {
        status_parts.push(Span::styled(
            " running…",
            Style::default().fg(Color::Yellow),
        ));
    }

    let status_line = Line::from(status_parts);
    frame.render_widget(
        Paragraph::new(status_line).block(Block::default().borders(Borders::NONE)),
        status_area,
    );

    // ── input area ──
    let prompt_w = 2u16;
    let input_w = input_area.width.saturating_sub(prompt_w + 1);
    let col = app.byte_to_col();
    // adjust input_scroll_x to keep cursor visible
    let scroll_x = if col < app.input_scroll_x {
        col
    } else if col >= app.input_scroll_x + input_w {
        col.saturating_sub(input_w) + 1
    } else {
        app.input_scroll_x
    };
    let visible_start = scroll_x as usize;
    let input_text: String = app.input.chars().skip(visible_start).collect();
    let input_trunc: String = input_text.chars().take(input_w as usize).collect();

    let input_widget = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(if input_trunc.is_empty() && !app.running {
            "type here…"
        } else {
            &input_trunc
        }),
    ]))
    .block(Block::default().borders(Borders::NONE))
    .style(if input_trunc.is_empty() && !app.running {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
    } else {
        Style::default()
    });

    frame.render_widget(input_widget, input_area);

    // Set cursor position (ratatui 0.30 uses Position struct)
    let cursor_col = prompt_w + col.saturating_sub(scroll_x);
    frame.set_cursor_position(ratatui::layout::Position::new(
        input_area.x + cursor_col.min(input_area.width.saturating_sub(1)),
        input_area.y,
    ));

    // ── hint line ──
    let hint = Line::from(Span::styled(
        "!bash  /command  @file  #memory",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(hint), hint_area);
}

// ── Agent setup ─────────────────────────────────────────────────

fn read_api_key() -> Option<String> {
    std::env::var("DEEPSEEK_API_KEY").ok()
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|s| !s.is_empty())
}

fn spawn_agent() -> anyhow::Result<(mpsc::UnboundedSender<String>, mpsc::UnboundedReceiver<StreamEvent>)> {
    let api_key = read_api_key()
        .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not set. Export it in your environment."))?;

    let config = AgentConfig::default();
    let model = config.default_model.clone();
    let llm = Arc::new(DeepSeekClient::new(api_key, &model)?);
    let registry = Arc::new(ToolRegistry::new());
    let sp = Arc::new(SystemPromptBuilder::new(config.clone()));

    // Register tools
    use aegis_tools::*;
    registry.register(Arc::new(BashTool::new()))?;
    registry.register(Arc::new(FileReadTool::new()))?;
    registry.register(Arc::new(FileEditTool::new()))?;
    registry.register(Arc::new(FileWriteTool::new()))?;
    registry.register(Arc::new(ListDirTool))?;
    registry.register(Arc::new(GlobTool::new()))?;
    registry.register(Arc::new(GrepTool::new()))?;
    registry.register(Arc::new(FileSearchTool))?;

    let mut agent = AgentLoop::new(config, llm, Arc::clone(&registry), sp);
    let scorer = Arc::new(aegis_core::llm::scorer::RuleBasedScorer);
    agent = agent.with_code_scorer(scorer);

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
    let (stream_tx, stream_rx) = mpsc::unbounded_channel::<StreamEvent>();

    tokio::spawn(async move {
        while let Some(text) = input_rx.recv().await {
            let tx = stream_tx.clone();
            let result = agent.run_streaming(&text, &move |event: StreamEvent| {
                let _ = tx.send(event);
            }).await;
            match result {
                Ok(output) => {
                    if !output.content.is_empty() {
                        let _ = stream_tx.send(StreamEvent::TextDelta(output.content));
                    }
                    let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                        content: None,
                        reasoning: None,
                        tool_uses: vec![],
                        stop_reason: Some("end_turn".into()),
                        usage: Default::default(),
                        model: String::new(),
                        latency_ms: 0,
                    }));
                }
                Err(e) => {
                    let _ = stream_tx.send(StreamEvent::TextDelta(format!("\n[Error] {e}\n")));
                    let _ = stream_tx.send(StreamEvent::Done(aegis_core::llm::client::LlmResponse {
                        content: None,
                        reasoning: None,
                        tool_uses: vec![],
                        stop_reason: Some("error".into()),
                        usage: Default::default(),
                        model: String::new(),
                        latency_ms: 0,
                    }));
                }
            }
        }
    });

    Ok((input_tx, stream_rx))
}

// ── Main ────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    install_panic_hook();

    // Build tokio runtime for agent background task
    let rt = tokio::runtime::Runtime::new()?;
    let _rt_guard = rt.enter(); // keep alive

    let (agent_tx, stream_rx) = match spawn_agent() {
        Ok(pair) => pair,
        Err(e) => {
            // No API key — run in demo mode
            eprintln!("Warning: {e}");
            eprintln!("Running in demo mode (no LLM backend).");
            let (tx, _rx) = mpsc::unbounded_channel();
            let (_, rx) = mpsc::unbounded_channel();
            (tx, rx)
        }
    };

    let _guard = TermGuard::enter()?;
    let out = stdout();
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".into());

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
        tokens_in: 0,
        tokens_out: 0,
        cost: 0.0,
        running: false,
        last_assist_idx: None,
    };

    let mut throttle = Throttle::new();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown);

    // Ctrl+C signal handler
    rt.spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown_flag.store(true, Ordering::SeqCst);
    });

    // Main loop
    while !app.quit && !shutdown.load(Ordering::SeqCst) {
        // ── 1. Poll terminal events ──
        if crossterm::event::poll(Duration::from_millis(10)).unwrap_or(false) {
            if let Ok(event) = crossterm::event::read() {
                match event {
                    CEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        match app.handle_key(k.code, k.modifiers) {
                            KeyAction::Submit => app.submit(),
                            KeyAction::Quit => app.quit = true,
                            KeyAction::None => {}
                        }
                        throttle.force = true;
                    }
                    CEvent::Resize(_, _) => {
                        term.clear()?;
                        throttle.force = true;
                    }
                    CEvent::Paste(text) => {
                        for ch in text.chars() { app.insert(ch); }
                        throttle.force = true;
                    }
                    CEvent::Mouse(me) => match me.kind {
                        crossterm::event::MouseEventKind::ScrollUp => { app.scroll_up(3); throttle.force = true; }
                        crossterm::event::MouseEventKind::ScrollDown => { app.scroll_down(3); throttle.force = true; }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }

        // ── 2. Drain agent stream events ──
        let mut got_stream = false;
        while let Ok(event) = app.stream_rx.try_recv() {
            app.handle_stream(event);
            got_stream = true;
        }
        if got_stream { throttle.force = true; }

        // ── 3. Render ──
        if throttle.should() {
            term.draw(|frame| render(frame, &mut app))?;
            throttle.mark();
        }
    }

    // Cleanup: reset cursor to visible position before exit
    let _ = stdout().execute(crossterm::cursor::Show);
    drop(_guard);
    Ok(())
}
