#![allow(dead_code, unused_variables)]
use ratatui::layout::Rect;
pub use tui_textarea::{CursorMove, TextArea};

pub struct InputState { pub textarea: TextArea<'static>, pub version: u32, pub lines_cache: Vec<String> }

fn refresh_cache(s: &mut InputState) { s.lines_cache = s.textarea.lines().to_vec(); }
pub struct InputSnapshot(String);
pub const PASTE_PLACEHOLDER_CHAR_THRESHOLD: usize = 10000;
pub struct InputGeometry { pub prompt: Rect, pub text: Rect, pub area: Rect }
pub fn compute_render_geometry(area: Rect, _hint_rows: usize) -> InputGeometry { InputGeometry { prompt: area, text: area, area } }

impl InputState {
    pub fn new() -> Self { let ta = TextArea::default(); let lc = ta.lines().to_vec(); Self { textarea: ta, version: 0, lines_cache: lc } }
    pub fn clear(&mut self) { self.textarea = TextArea::default(); self.lines_cache.clear(); }
    pub fn cursor(&self) -> (usize, usize) { self.textarea.cursor() }
    pub fn cursor_col(&self) -> usize { self.textarea.cursor().1 }
    pub fn cursor_row(&self) -> usize { self.textarea.cursor().0 }
    pub fn set_cursor(&mut self, row: usize, col: usize) { self.textarea.move_cursor(CursorMove::Jump(row as u16, col as u16)); }
    pub fn set_cursor_col(&mut self, col: usize) { let (r, _) = self.cursor(); self.textarea.move_cursor(CursorMove::Jump(r as u16, col as u16)); }
    pub fn move_home(&mut self) { self.textarea.move_cursor(CursorMove::Head); }
    pub fn editor(&self) -> ratatui::widgets::Paragraph<'static> { ratatui::widgets::Paragraph::new(self.lines_cache.join("\n")) }
    pub fn is_empty(&self) -> bool { self.lines_cache.iter().all(|l| l.is_empty()) }
    pub fn lines(&self) -> &[String] { &self.lines_cache }
    pub fn text(&self) -> String { self.lines_cache.join("\n") }
    pub fn snapshot(&self) -> InputSnapshot { InputSnapshot(self.text()) }
    pub fn set_text(&mut self, s: &str) { self.textarea = TextArea::from([s]); self.lines_cache = s.lines().map(|l| l.to_string()).collect(); }
    pub fn insert_str(&mut self, s: &str) { self.textarea.insert_str(s); refresh_cache(self); }
    pub fn append_to_active_paste_block(&mut self, s: &str) -> bool { self.textarea.insert_str(s); refresh_cache(self); true }
    pub fn delete_image_badge(&mut self, _direction: &str) -> Option<usize> { None }
    pub fn insert_paste_block(&mut self, s: &str) { self.textarea.insert_str(s); refresh_cache(self); }
    pub fn renumber_image_badges(&mut self) {}
    pub fn replace_lines_and_cursor(&mut self, lines: Vec<String>, _row: usize, _col: usize) { let s: Vec<&str> = lines.iter().map(|s| s.as_str()).collect(); self.textarea = TextArea::from(s); self.lines_cache = lines; }
    pub fn restore_snapshot(&mut self, snap: InputSnapshot) { self.set_text(&snap.0); }
    pub fn textarea_delete_char_after(&mut self) -> bool { let r = self.textarea.delete_char(); refresh_cache(self); r }
    pub fn textarea_delete_char_before(&mut self) -> bool { let r = self.textarea.delete_char(); refresh_cache(self); r }
    pub fn textarea_delete_line_after(&mut self) -> bool { false }
    pub fn textarea_delete_line_before(&mut self) -> bool { false }
    pub fn textarea_delete_word_after(&mut self) -> bool { false }
    pub fn textarea_delete_word_before(&mut self) -> bool { false }
    pub fn textarea_insert_char(&mut self, c: char) { self.textarea.insert_char(c); refresh_cache(self); }
    pub fn textarea_insert_newline(&mut self) -> bool { let r = self.textarea.insert_str("\n"); refresh_cache(self); r }
    pub fn textarea_move_down(&mut self) -> bool { self.textarea.move_cursor(CursorMove::Down); false }
    pub fn textarea_move_end(&mut self) -> bool { self.textarea.move_cursor(CursorMove::End); false }
    pub fn textarea_move_home(&mut self) -> bool { self.textarea.move_cursor(CursorMove::Head); false }
    pub fn textarea_move_left(&mut self) -> bool { self.textarea.move_cursor(CursorMove::Back); false }
    pub fn textarea_move_right(&mut self) -> bool { self.textarea.move_cursor(CursorMove::Forward); false }
    pub fn textarea_move_up(&mut self) -> bool { self.textarea.move_cursor(CursorMove::Up); false }
    pub fn textarea_move_word_left(&mut self) -> bool { self.textarea.move_cursor(CursorMove::WordBack); false }
    pub fn textarea_move_word_right(&mut self) -> bool { self.textarea.move_cursor(CursorMove::WordForward); false }
    pub fn textarea_redo(&mut self) -> bool { false }
    pub fn textarea_undo(&mut self) -> bool { false }
    pub fn textarea_yank(&mut self) -> bool { false }
}

pub fn parse_paste_placeholder_before_cursor(_line: &str, _col: usize) -> Option<usize> { None }
pub fn count_text_chars(s: &str) -> usize { s.chars().count() }
pub fn visual_line_count(_app: &crate::app::App, _width: u16) -> u16 { 1 }
pub fn prompt_prefix_text() -> String { "> ".into() }
pub fn configure_input_textarea(_app: &mut crate::app::App) {}
pub fn render_text_input_field(_label: &str, _val: &str, _ph: &str) -> String { String::new() }
pub fn add_marketplace_example_lines(_lines: &mut Vec<String>) {}
pub fn text_input_line(_prefix: &str, _cursor: usize, _ph: &str) -> String { String::new() }
