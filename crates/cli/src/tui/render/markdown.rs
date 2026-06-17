//! TUI rendering — markdown, syntax highlighting, message cards.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};




// ── Rendering ───────────────────────────────────────────────────

pub(crate) fn render_markdown(text: &str, area_w: u16) -> Vec<Line<'static>> {
    use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};

    let width = (area_w.saturating_sub(8)).max(20) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut current = String::new();
    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let mut table_alignments: Vec<Alignment> = vec![];
    let mut table_rows: Vec<Vec<String>> = vec![];
    let mut current_row: Vec<String> = vec![];
    let mut in_table_cell = false;

    let mut opts = Options::ENABLE_STRIKETHROUGH;
    opts.insert(Options::ENABLE_TABLES);

    for event in Parser::new_ext(text, opts) {
        match event {
            // ── Code blocks ──
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_line(&mut current, &mut lines, width);
                in_code_block = true;
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = lang.as_ref().trim();
                        if l.is_empty() { None } else { Some(l.to_string()) }
                    }
                    _ => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                flush_code_block_with_hl(&mut current, &mut lines, width, code_lang.as_deref());
                in_code_block = false;
                code_lang = None;
            }

            // ── Tables ──
            Event::Start(Tag::Table(alignments)) => {
                flush_line(&mut current, &mut lines, width);
                table_alignments = alignments;
                table_rows.clear();
            }
            Event::Start(Tag::TableHead) => {}
            Event::End(TagEnd::TableHead) => {}
            Event::Start(Tag::TableRow) => { current_row.clear(); }
            Event::End(TagEnd::TableRow) => {
                table_rows.push(std::mem::take(&mut current_row));
            }
            Event::Start(Tag::TableCell) => { in_table_cell = true; }
            Event::End(TagEnd::TableCell) => {
                in_table_cell = false;
                current_row.push(std::mem::take(&mut current));
            }
            Event::End(TagEnd::Table) => {
                render_table(&table_rows, &table_alignments, width, &mut lines);
                lines.push(Line::from(""));
                table_rows.clear();
                table_alignments.clear();
            }

            // ── Headings ──
            Event::Start(Tag::Heading { level, .. }) => {
                flush_line(&mut current, &mut lines, width);
                current.push_str(&"#".repeat(level as usize));
                current.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => {
                let h = std::mem::take(&mut current);
                lines.push(Line::from(Span::styled(h, Style::default().add_modifier(Modifier::BOLD))));
            }

            // ── Lists ──
            Event::Start(Tag::Item) => { flush_line(&mut current, &mut lines, width); current.push_str("  • "); }
            Event::Start(Tag::BlockQuote(_)) => { current.push('▎'); }

            // ── Inline ──
            Event::Text(t) => { current.push_str(&t); }
            Event::Code(t) => {
                if in_code_block || in_table_cell { current.push_str(&t); }
                else { current.push_str(&format!(" `{t}` ")); }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block { current.push('\n'); }
                else { flush_line(&mut current, &mut lines, width); }
            }
            Event::Start(Tag::Strong) => { current.push_str("**"); }
            Event::End(TagEnd::Strong) => { current.push_str("**"); }
            Event::Start(Tag::Emphasis) => { current.push('*'); }
            Event::End(TagEnd::Emphasis) => { current.push('*'); }

            // ── Horizontal rule ──
            Event::Rule => {
                flush_line(&mut current, &mut lines, width);
                lines.push(Line::from(Span::styled("─".repeat(width.min(40)), Style::default().fg(Color::DarkGray))));
            }

            _ => {}
        }
    }
    flush_line(&mut current, &mut lines, width);
    lines
}

pub(crate) fn render_table(rows: &[Vec<String>], _alignments: &[pulldown_cmark::Alignment], width: usize, lines: &mut Vec<Line<'static>>) {
    if rows.is_empty() { return; }
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    // Calculate column widths
    let mut col_widths: Vec<usize> = vec![0; ncols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            col_widths[i] = col_widths[i].max(cell.chars().count().min(30));
        }
    }
    // Constrain to fit
    let total: usize = col_widths.iter().sum::<usize>() + (ncols - 1) * 3;
    if total > width {
        // Scale down proportionally
        let scale = width as f32 / total as f32;
        for w in &mut col_widths { *w = (*w as f32 * scale) as usize; }
    }

    let header_style = Style::default().add_modifier(Modifier::BOLD);
    let sep_style = Style::default().fg(Color::DarkGray);

    for (row_idx, row) in rows.iter().enumerate() {
        let is_header = row_idx == 0;
        let mut line_spans = vec![];
        for (i, cell) in row.iter().enumerate() {
            if i > 0 { line_spans.push(Span::raw(" │ ")); }
            let w = *col_widths.get(i).unwrap_or(&10);
            let display: String = if cell.chars().count() > w {
                cell.chars().take(w.saturating_sub(1)).collect::<String>() + "..."
            } else {
                format!("{:width$}", cell, width = w)
            };
            let style = if is_header { header_style } else { Style::default() };
            line_spans.push(Span::styled(display, style));
        }
        lines.push(Line::from(line_spans));
        // Separator after header
        if is_header && rows.len() > 1 {
            let mut sep_spans = vec![];
            for i in 0..ncols {
                if i > 0 { sep_spans.push(Span::raw("─┼─")); }
                let w = *col_widths.get(i).unwrap_or(&10);
                sep_spans.push(Span::styled("─".repeat(w), sep_style));
            }
            lines.push(Line::from(sep_spans));
        }
    }
}

fn flush_line(current: &mut String, lines: &mut Vec<Line<'static>>, w: usize) {
    let text = std::mem::take(current);
    if text.trim().is_empty() { return; }
    for wrapped in textwrap::wrap(&text, w) {
        lines.push(parse_inline_markdown(&wrapped));
    }
}

pub(crate) fn flush_code_block_with_hl(current: &mut String, lines: &mut Vec<Line<'static>>, w: usize, lang: Option<&str>) {
    let text = std::mem::take(current);
    if text.is_empty() { return; }
    let border_fg = Color::Rgb(60, 60, 70);
    let bg = Color::Rgb(28, 28, 36);
    let pad = 2usize;

    // Top border with optional language label
    let top_label = if let Some(l) = lang {
        format!(" ┌── {} ─{}", l, "─".repeat(w.saturating_sub(6).saturating_sub(l.len()).min(35)))
    } else {
        format!(" ┌{}", "─".repeat(w.saturating_sub(4).min(40)))
    };
    lines.push(Line::from(Span::styled(top_label, Style::default().fg(border_fg).bg(bg))));

    // Highlighted content with per-line colored spans
    let highlighted_lines = highlight_code_lines(&text, lang);
    for spans in &highlighted_lines {
        let mut line_spans: Vec<Span> = vec![Span::styled(" │", Style::default().fg(border_fg).bg(bg))];
        let mut char_count = 0usize;
        for (s, t) in spans {
            if char_count + t.len() > w.saturating_sub(pad + 2) {
                let remaining = w.saturating_sub(pad + 2).saturating_sub(char_count);
                if remaining > 0 {
                    line_spans.push(Span::styled(t[..remaining.min(t.len())].to_string(), *s));
                }
                break;
            }
            line_spans.push(Span::styled(t.to_string(), *s));
            char_count += t.len();
        }
        lines.push(Line::from(line_spans));
    }

    // Bottom border
    lines.push(Line::from(Span::styled(format!(" └{}", "─".repeat(w.saturating_sub(4).min(40))), Style::default().fg(border_fg).bg(bg))));
}

/// Syntax highlight code using syntect. Returns Vec of (Style, text) per line.
pub(crate) fn highlight_code_lines(code: &str, lang: Option<&str>) -> Vec<Vec<(Style, String)>> {
    use syntect::highlighting::ThemeSet;
    use syntect::parsing::SyntaxSet;
    use std::sync::OnceLock;

    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    static THEME: OnceLock<syntect::highlighting::Theme> = OnceLock::new();

    let ss = SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines);
    let theme = THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes.get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
            .cloned()
            .unwrap_or_else(syntect::highlighting::Theme::default)
    });

    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .or_else(|| lang.and_then(|l| ss.find_syntax_by_extension(l)))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut result: Vec<Vec<(Style, String)>> = Vec::new();

    for line in code.lines() {
        let mut spans: Vec<(Style, String)> = Vec::new();
        if let Ok(ranges) = highlighter.highlight_line(line, ss) {
            for (hl_style, text) in &ranges {
                let ratatui_style = syntect_style_to_ratatui(*hl_style);
                spans.push((ratatui_style, text.to_string()));
            }
        } else {
            spans.push((Style::default(), line.to_string()));
        }
        result.push(spans);
    }

    result
}

/// Convert syntect Style to ratatui Style.
fn syntect_style_to_ratatui(s: syntect::highlighting::Style) -> Style {
    let fg = syntect_color_to_ratatui(s.foreground);
    let mut style = Style::default().fg(fg);
    if s.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

/// Convert syntect Color to ratatui Color.
fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn parse_inline_markdown(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                spans.push(Span::styled(rest[..end].to_string(), Style::default().add_modifier(Modifier::BOLD)));
                remaining = &rest[end+2..];
            } else {
                spans.push(Span::raw("**"));
                remaining = rest;
            }
        } else if let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                spans.push(Span::styled(rest[..end].to_string(), Style::default().add_modifier(Modifier::ITALIC)));
                remaining = &rest[end+1..];
            } else {
                spans.push(Span::raw("*"));
                remaining = rest;
            }
        } else if let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(rest[..end].to_string(), Style::default().fg(Color::Yellow).bg(Color::Rgb(40, 40, 40))));
                remaining = &rest[end+1..];
            } else {
                spans.push(Span::raw("`"));
                remaining = rest;
            }
        } else {
            let next = remaining.find(['*', '`']).unwrap_or(remaining.len());
            if next > 0 || spans.is_empty() {
                spans.push(Span::raw(remaining[..next].to_string()));
            }
            remaining = &remaining[next..];
        }
    }
    Line::from(spans)
}
