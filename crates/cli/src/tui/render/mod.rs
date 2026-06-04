//! TUI rendering — markdown, syntax highlighting, message cards.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use crate::{Msg, App};
use crate::EFFORTS;


mod markdown;
pub(crate) use markdown::render_markdown;

pub(crate) fn render_msg<'a>(out: &mut Vec<Line<'a>>, msg: &'a Msg, area_w: u16) {
    let w = area_w.saturating_sub(4) as usize;
    let red_bg = Style::default().bg(Color::Rgb(80, 20, 20));
    let green_bg = Style::default().bg(Color::Rgb(20, 60, 20));
    let dark_gray = Style::default().fg(Color::DarkGray);
    let _code_bg = Style::default().bg(Color::Rgb(30, 30, 35));

    match msg {
        Msg::User(text) => {
            out.push(Line::from(""));
            out.push(Line::from(Span::styled(
                "▸ You",
                Style::default().fg(Color::Cyan).bg(Color::Rgb(55, 55, 65)).add_modifier(Modifier::BOLD),
            )));
            for line in text.lines() {
                for wrapped in textwrap::wrap(line, w.max(20)) {
                    out.push(Line::from(Span::raw(wrapped.to_string())));
                }
            }
            out.push(Line::from(""));
        }
        Msg::Asst { text, think } => {
            if !think.is_empty() {
                out.push(Line::from(""));
                out.push(Line::from(Span::styled(
                    "  [Thinking] (collapsed)",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )));
            }
            if !text.is_empty() {
                let dot = Span::styled("*", Style::default().fg(Color::Blue));
                let md_lines = render_markdown(text, area_w);
                if md_lines.is_empty() {
                    out.push(Line::from(vec![dot, Span::raw(" ")]));
                } else {
                    let max_search_lines = 15usize; // fold long search results
                    let total_lines = md_lines.len();
                    for (i, mut line) in md_lines.into_iter().enumerate() {
                        if i == 0 {
                            let mut spans = vec![dot.clone(), Span::raw(" ")];
                            spans.extend(line.spans);
                            line = Line::from(spans);
                        }
                        // Search result folding (A3)
                        if i == max_search_lines && total_lines > max_search_lines + 3 {
                            out.push(Line::from(Span::styled(
                                format!("  ... {} more lines (scroll to see)", total_lines - max_search_lines),
                                dark_gray.add_modifier(Modifier::ITALIC),
                            )));
                        }
                        if i >= max_search_lines { continue; }
                        out.push(line);
                    }
                }
            }
            // Turn separator (C3)
            out.push(Line::from(Span::styled(
                "─".repeat(area_w.saturating_sub(4).max(10) as usize),
                Style::default().fg(Color::Rgb(40, 40, 40)),
            )));
        }
        Msg::Tool { name, done, ok, detail, elapsed_ms } => {
            let icon = if *done { if *ok { "+" } else { "x" } } else { "..." };
            let color = if *done { if *ok { Color::Green } else { Color::Red } } else { Color::Yellow };
            let _error_bg = Style::default().bg(Color::Rgb(80, 20, 20));

            // Elapsed time right-aligned
            let time_str = if *elapsed_ms > 0 {
                let secs = *elapsed_ms as f64 / 1000.0;
                if secs >= 1.0 { format!("{:.1}s", secs) } else { format!("{}ms", elapsed_ms) }
            } else {
                String::new()
            };

            let name_style = if *done && !*ok {
                Style::default().fg(Color::Red).bg(Color::Rgb(80, 20, 20)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            };

            out.push(Line::from(vec![
                Span::styled(format!("  {icon} {name}"), name_style),
                Span::styled(format!(" {}", time_str), dark_gray),
                Span::styled(format!("  {:.100}", detail), Style::default().fg(Color::Gray)),
            ]));

            // Diff coloring for file_edit results
            if *done && *ok && *name == "file_edit" {
                for line in detail.lines() {
                    let display = &line[..line.len().min(area_w as usize - 6)];
                    if line.starts_with('+') && !line.starts_with("+++") {
                        out.push(Line::from(Span::styled(format!("    {display}"), green_bg)));
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        out.push(Line::from(Span::styled(format!("    {display}"), red_bg)));
                    }
                }
            }

            // Error detail with red background for failed tools
            if *done && !*ok {
                for line in detail.lines() {
                    out.push(Line::from(Span::styled(
                        format!("    {}", &line[..line.len().min(area_w as usize - 6)]),
                        Style::default().fg(Color::Rgb(255, 150, 150)).bg(Color::Rgb(60, 15, 15)),
                    )));
                }
            }
        }
        Msg::System(text) => {
            for line in text.lines() {
                out.push(Line::from(Span::styled(line, Style::default().fg(Color::Yellow))));
            }
        }
    }
}

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 20 || area.height < 8 { return; }

    let dialog_active = app.dialog.is_some() || app.model_dialog.is_some() || app.skill_dialog.is_some() || app.session_dialog.is_some();
    let dialog_h: u16 = if let Some(ref dlg) = app.dialog {
        (dlg.options.len() as u16 + 6).min(16)
    } else if app.model_dialog.is_some() {
        8
    } else if let Some(ref sd) = app.skill_dialog {
        (sd.skills.len() as u16 + 4).min(20)
    } else if let Some(ref sd) = app.session_dialog {
        (sd.sessions.len() as u16 + 4).min(18)
    } else { 0 };
    let input_lines_count = app.input.lines().count().max(1);
    let input_h: u16 = if dialog_active { 0 } else {
        2 + input_lines_count.min(8) as u16
    };
    let status_h: u16 = if dialog_active || (!app.running && app.last_turn_ms == 0) { 0 } else { 1 };
    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(dialog_h),
        Constraint::Length(status_h),
        Constraint::Length(input_h),
        Constraint::Length(1),
    ]);
    let [msg_area, dialog_area, status_area, input_outer, footer_area] = layout.areas(area);

    // ── messages ──
    app.viewport_height = msg_area.height.saturating_sub(1) as usize;
    app.msg_area_y = msg_area.y;
    app.msg_area_h = msg_area.height;

    // ── status line (timer + live tokens above input) ──
    if status_h > 0 {
        let mut status_text = String::new();
        if app.running {
            if let Some(start) = app.turn_start {
                let secs = start.elapsed().as_secs();
                if app.turn_tokens_in > 0 || app.turn_tokens_out > 0 {
                    status_text = format!(
                        "  {}s · in:{} out:{}",
                        secs, fmt_tokens(app.turn_tokens_in), fmt_tokens(app.turn_tokens_out)
                    );
                } else {
                    status_text = format!("  Thinking... {}s", secs);
                }
            }
        } else if app.last_turn_ms > 0 {
            let cache_str = if app.turn_tokens_cache > 0 {
                format!(" · cache:{}", fmt_tokens(app.turn_tokens_cache))
            } else { String::new() };
            status_text = format!(
                "  [OK] {}s · in:{} out:{}{}",
                app.last_turn_ms / 1000,
                fmt_tokens(app.turn_tokens_in),
                fmt_tokens(app.turn_tokens_out),
                cache_str,
            );
        }
        if !status_text.is_empty() {
            let style = if app.running {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(&status_text, style))),
                status_area,
            );
        }
    }
    let mut lines: Vec<Line> = Vec::with_capacity(256);
    let mut line_texts: Vec<String> = Vec::with_capacity(256);

    if app.messages.is_empty() {
        for &wl in &[
            "Welcome to aegis.  Type a question and press Enter.",
            "",
            "  Esc                  quit (empty input) / clear input",
            "  Ctrl+C               copy selection (drag to select)",
            "  Ctrl+D               quit",
            "  PgUp / PgDn          scroll messages",
        ] {
            lines.push(Line::from(Span::styled(wl, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))));
            line_texts.push(wl.to_string());
        }
    }

    // Render messages with parallel tool grouping (B2)
    let mut i = 0;
    while i < app.messages.len() {
        let msg = &app.messages[i];

        // Detect parallel tool group: consecutive Msg::Tool with done=false or all done
        if matches!(msg, Msg::Tool { .. }) {
            let mut group_end = i + 1;
            while group_end < app.messages.len() && matches!(&app.messages[group_end], Msg::Tool { .. }) {
                group_end += 1;
            }
            if group_end > i + 1 {
                // Parallel tool group: render with shared border
                let before = lines.len();
                lines.push(Line::from(Span::styled(
                    "  ┌─ Parallel tools ─",
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
                for j in i..group_end {
                    render_msg(&mut lines, &app.messages[j], msg_area.width);
                }
                lines.push(Line::from(Span::styled(
                    "  └─",
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
                for line in &lines[before..] {
                    line_texts.push(line.spans.iter().map(|s| s.content.as_ref()).collect::<String>());
                }
                i = group_end;
                continue;
            }
        }

        let before = lines.len();
        render_msg(&mut lines, msg, msg_area.width);
        for line in &lines[before..] {
            line_texts.push(line.spans.iter().map(|s| s.content.as_ref()).collect::<String>());
        }
        i += 1;
    }

    if app.running {
        lines.push(Line::from(Span::styled("|", Style::default().fg(Color::Yellow))));
        line_texts.push("|".to_string());
    }
    let was_at_bottom = app.at_bottom();
    app.lines_buf = line_texts;
    if was_at_bottom {
        app.scroll = app.total_lines().saturating_sub(app.viewport_height);
    }

    let total = lines.len();
    let start = app.scroll.min(total);
    let end = (start + app.viewport_height).min(total);
    let mut visible: Vec<Line> = if start < end { lines[start..end].to_vec() } else { vec![] };

    if let (Some(a), Some(b)) = (app.sel_start, app.sel_end) {
        let lo = a.min(b); let hi = a.max(b);
        let sel_style = Style::default().add_modifier(Modifier::REVERSED);
        for i in 0..visible.len() {
            let abs_idx = start + i;
            if abs_idx >= lo && abs_idx <= hi {
                visible[i] = Line::from(visible[i].spans.iter().map(|s| {
                    Span::styled(s.content.clone(), s.style.patch(sel_style))
                }).collect::<Vec<Span>>());
            }
        }
    }

    let scroll_hint = if total > app.viewport_height && !app.at_bottom() {
        format!(" ^ {} lines above (PgUp/PgDn) ", app.scroll)
    } else { String::new() };

    frame.render_widget(Paragraph::new(Text::from(visible)), msg_area);
    if !scroll_hint.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(scroll_hint, Style::default().fg(Color::Yellow).bg(Color::Black)))).right_aligned(),
            Rect { x: msg_area.x, y: msg_area.y + msg_area.height.saturating_sub(1), width: msg_area.width, height: 1 },
        );
    }

    // ── dialog (ask_user popup) ──
    if let Some(ref dlg) = app.dialog {
        let d_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {}", dlg.header))
            .border_style(Style::default().fg(Color::Yellow));
        let mut d_lines: Vec<Line> = Vec::new();
        d_lines.push(Line::from(Span::styled(&dlg.question, Style::default().fg(Color::White))));
        d_lines.push(Line::from(""));
        let total = dlg.options.len() + 2; // options + custom + cancel
        for i in 0..total {
            let sel = dlg.selected == i;
            let prefix = if sel { ">" } else { " " };
            let style = if sel {
                Style::default().fg(Color::Black).bg(Color::Rgb(200, 200, 200))
            } else {
                Style::default()
            };
            if i < dlg.options.len() {
                d_lines.push(Line::from(vec![
                    Span::styled(format!("{prefix} [{}] ", i + 1), style.add_modifier(Modifier::BOLD)),
                    Span::styled(&dlg.options[i], style),
                ]));
            } else if i == dlg.options.len() {
                // Custom input option
                if dlg.in_custom {
                    d_lines.push(Line::from(vec![
                        Span::styled(format!("{prefix}    "), style),
                        Span::raw("> "),
                        Span::raw(&dlg.custom_input),
                    ]));
                } else {
                    d_lines.push(Line::from(vec![
                        Span::styled(format!("{prefix}    "), style),
                        Span::styled("提供其他答案...", style.fg(Color::DarkGray)),
                    ]));
                }
            } else {
                d_lines.push(Line::from(vec![
                    Span::styled(format!("{prefix}    "), style),
                    Span::styled("取消 (Esc)", style.fg(Color::DarkGray)),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(Text::from(d_lines)).block(d_block), dialog_area);
    }

    if let Some(ref md) = app.model_dialog {
        let d_block = Block::default()
            .borders(Borders::ALL)
            .title(" Model & Thinking ")
            .border_style(Style::default().fg(Color::Cyan));
        let mut d_lines: Vec<Line> = Vec::new();
        d_lines.push(Line::from(""));
        for (i, (_id, name)) in md.models.iter().enumerate() {
            let sel = md.model_idx == i;
            let prefix = if sel { ">" } else { " " };
            let style = if sel { Style::default().fg(Color::Black).bg(Color::Rgb(180, 220, 255)) } else { Style::default() };
            d_lines.push(Line::from(vec![
                Span::styled(format!("{prefix} {name}", ), style.add_modifier(Modifier::BOLD)),
            ]));
        }
        d_lines.push(Line::from(""));
        let effort_str = format!("Thinking: {}  < >", EFFORTS[md.effort_idx]);
        d_lines.push(Line::from(Span::styled(effort_str, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        d_lines.push(Line::from(Span::styled("Enter=confirm  Esc=cancel", Style::default().fg(Color::DarkGray))));
        frame.render_widget(Paragraph::new(Text::from(d_lines)).block(d_block), dialog_area);
    }

    if let Some(ref sd) = app.skill_dialog {
        let d_block = Block::default()
            .borders(Borders::ALL)
            .title(" Skills (up/down select, Enter to invoke, Esc to cancel) ")
            .border_style(Style::default().fg(Color::Magenta));
        let mut d_lines: Vec<Line> = Vec::new();
        for (i, (name, desc)) in sd.skills.iter().enumerate() {
            let sel = sd.skill_idx == i;
            let prefix = if sel { ">" } else { " " };
            let style = if sel { Style::default().fg(Color::Black).bg(Color::Rgb(230, 200, 255)) } else { Style::default() };
            let desc_short: String = desc.chars().take(60).collect();
            d_lines.push(Line::from(vec![
                Span::styled(format!("{prefix} /{}", name), style.add_modifier(Modifier::BOLD)),
                Span::styled(format!(" — {}", desc_short), Style::default().fg(Color::Gray)),
            ]));
        }
        frame.render_widget(Paragraph::new(Text::from(d_lines)).block(d_block), dialog_area);
    }

    // ── command hint area ──
    let mut hint_text = String::new();
    if !dialog_active && !app.input.is_empty() && app.input.starts_with('/') {
        let prefix = &app.input[1..]; // strip '/'
        let space_pos = prefix.find(' ').unwrap_or(prefix.len());
        let cmd_name = &prefix[..space_pos];
        let matches = App::command_matches(cmd_name);
        if !matches.is_empty() && cmd_name.len() >= 1 {
            hint_text = matches.iter().take(8).map(|c| format!("/{c}")).collect::<Vec<_>>().join("  ");
        }
    }

    // ── input area ──
    if !dialog_active {
        app.input_y = input_outer.y;
        app.input_h = input_outer.height;

        let prompt = "▸ ";
        let lines: Vec<&str> = app.input.lines().collect();
        let mut display_lines: Vec<Line> = Vec::new();
        let purple = Style::default().fg(Color::Magenta);
        let normal = Style::default();

        for (i, line) in lines.iter().enumerate() {
            if i == 0 {
                let mut spans: Vec<Span> = Vec::new();
                if app.input.starts_with('/') {
                    let space_pos = line.find(' ').unwrap_or(line.len());
                    let cmd_part = &line[..space_pos];
                    let cmd_name = cmd_part.trim_start_matches('/');
                    let rest = &line[space_pos..];
                    spans.push(Span::styled(format!("{prompt}"), normal));
                    if App::is_valid_command(cmd_name) {
                        spans.push(Span::styled(cmd_part.to_string(), purple));
                    } else {
                        spans.push(Span::styled(cmd_part.to_string(), normal));
                    }
                    if !rest.is_empty() {
                        spans.push(Span::styled(rest.to_string(), normal));
                    }
                } else {
                    spans.push(Span::styled(format!("{prompt}{line}"), normal));
                }
                display_lines.push(Line::from(spans));
            } else {
                display_lines.push(Line::from(Span::styled(format!("  {line}"), normal)));
            }
        }
        let input_text = Text::from(display_lines);
        let block = Block::default().borders(Borders::TOP.union(Borders::BOTTOM)).border_style(Style::default().fg(Color::DarkGray));
        frame.render_widget(Paragraph::new(input_text).block(block), input_outer);

        // Calculate cursor position (for multiline support)
        let before_cursor = &app.input[..app.cursor_byte.min(app.input.len())];
        let current_line_idx = before_cursor.matches('\n').count();
        let line_start = before_cursor.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_in_line = unicode_width::UnicodeWidthStr::width(&before_cursor[line_start..]) as u16;
        let line_offset = if current_line_idx == 0 { unicode_width::UnicodeWidthStr::width(prompt) as u16 } else { 2u16 };
        let cursor_x = input_outer.x + line_offset + col_in_line;
        let cursor_y = input_outer.y + 1 + current_line_idx as u16;
        frame.set_cursor_position(ratatui::layout::Position::new(
            cursor_x.min(input_outer.right().saturating_sub(2)),
            cursor_y,
        ));
    }

    // ── footer ──
    let ctx_max = 1_048_576u64;
    let ctx_pct: f64 = if app.tokens_in > 0 { app.tokens_in as f64 / ctx_max as f64 * 100.0 } else { 0.0 };

    // Command hints (shown when typing /)
    if !hint_text.is_empty() {
        let hint_para = Paragraph::new(Line::from(Span::styled(
            hint_text,
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(hint_para, footer_area);
        // Don't render normal footer when showing hints
    } else {
        // Footer: mode (left, visible) + hints (left, dim) ... right: model eff ctx cost timer
        let mode_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        let left_text = format!("[{}]  !bash /cmd @file #mem", app.mode);
        let left_spans = vec![
            Span::styled(format!("[{}]", app.mode), mode_style),
            Span::raw("  "),
            Span::styled("!bash /cmd @file #mem", Style::default().fg(Color::DarkGray)),
    ];

    let mut right_parts: Vec<Span> = Vec::new();
    right_parts.push(Span::styled(format!("{} {}", app.model, app.reasoning_effort), Style::default().fg(Color::Gray)));

    // Always show context bar
    let bar_w: usize = 8;
    let filled: usize = ((ctx_pct as f64 / 100.0 * bar_w as f64) as usize).min(bar_w);
    let empty: usize = bar_w - filled;
    let bar_color = if ctx_pct > 80.0 { Color::Red } else if ctx_pct > 50.0 { Color::Yellow } else { Color::DarkGray };
    right_parts.push(Span::styled(format!(" {}K/{}K", app.tokens_in / 1000, ctx_max / 1000), Style::default().fg(Color::Gray)));
    right_parts.push(Span::styled(format!(" [{}{}]({:.0}%)", "█".repeat(filled), "░".repeat(empty), ctx_pct), Style::default().fg(bar_color)));
    let cs = fmt_cost(app.cost);
    if !cs.is_empty() { right_parts.push(Span::raw(format!(" {cs}"))); }
    let _cache_pct = app.last_call_cache_pct;
    right_parts.push(Span::styled(format!(" cache {:.1}%", app.last_call_cache_pct), Style::default().fg(if app.last_call_cache_pct > 80.0 { Color::Green } else if app.last_call_cache_pct > 40.0 { Color::Yellow } else { Color::Red })));
    let left_w = unicode_width::UnicodeWidthStr::width(left_text.as_str()) as u16;
    let right_text: String = right_parts.iter().map(|s| s.content.as_ref()).collect();
    let right_w = unicode_width::UnicodeWidthStr::width(right_text.as_str()) as u16;
    let padding = footer_area.width.saturating_sub(left_w + right_w);
    let mut fspans = left_spans;
    fspans.push(Span::raw(" ".repeat(padding as usize)));
    fspans.extend(right_parts);
    frame.render_widget(Paragraph::new(Line::from(fspans)), footer_area);
    }
}

pub(crate) fn fmt_tokens(n: u64) -> String {
    if n >= 100_000 { format!("{}K", n / 1000) }
    else if n >= 1000 { format!("{:.1}K", n as f64 / 1000.0) }
    else { format!("{n}") }
}

pub(crate) fn fmt_cost(c: f64) -> String {
    if c >= 0.01 { format!("¥{:.2}", c) }
    else if c > 0.0 { format!("¥{:.4}", c) }
    else { String::new() }
}

