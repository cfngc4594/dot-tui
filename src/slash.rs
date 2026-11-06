//! Grok slash snapshot/row contract and wrapped navigation; Dot's only local
//! command is /language. No Grok registry, agent actions or message translation.
use crate::{
    i18n::{self, Language},
    slash_dropdown::{self, RenderedDropdown},
    theme::Theme,
};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
};
pub const MAX_VISIBLE_SUGGESTIONS: usize = 8;
#[derive(Clone, Debug)]
pub struct CommandProvenance;
impl CommandProvenance {
    pub fn badge(&self) -> Cow<'static, str> {
        Cow::Borrowed("")
    }
}
#[derive(Clone, Debug)]
pub struct SuggestionRow {
    pub display: String,
    pub description: String,
    pub insert_text: String,
    pub indices: Vec<u32>,
    pub tag: Option<String>,
    pub provenance: Option<CommandProvenance>,
}
#[derive(Default)]
pub struct SlashSnapshot {
    pub open: bool,
    pub matches: Vec<SuggestionRow>,
    pub selected: usize,
}
#[derive(Default)]
pub struct Menu {
    pub snapshot: SlashSnapshot,
    argument: bool,
    hover: Option<usize>,
    last: String,
    dismissed: Option<String>,
    pub area: Cell<Rect>,
    panel: Cell<Rect>,
    hit: RefCell<RenderedDropdown>,
}
pub enum Choice {
    Arguments,
    Language(Language),
}
fn row(display: &str, description: &str, insert: &str) -> SuggestionRow {
    SuggestionRow {
        display: display.into(),
        description: description.into(),
        insert_text: insert.into(),
        indices: vec![],
        tag: None,
        provenance: None,
    }
}
pub fn local(text: &str) -> bool {
    let t = text.trim();
    t == "/" || t == "/language" || t.starts_with("/language ")
}
impl Menu {
    pub fn open(&self) -> bool {
        self.snapshot.open
    }
    pub fn sync(&mut self, text: &str, cursor: usize, enabled: bool) {
        let text = if enabled && cursor == text.len() {
            text
        } else {
            ""
        };
        let changed = text != self.last;
        if changed {
            self.snapshot.selected = 0;
            self.hover = None;
            self.last = text.into();
            self.dismissed = None;
        }
        self.snapshot.matches.clear();
        self.snapshot.open = false;
        if !text.starts_with('/') || text.contains('\n') || self.dismissed.as_deref() == Some(text)
        {
            return;
        }
        let query = &text[1..];
        self.argument = query.starts_with("language ");
        if self.argument {
            let arg = query[9..].trim().to_lowercase();
            for (display, insert) in [("English", "en"), ("中文", "zh")] {
                if arg.is_empty()
                    || display.to_lowercase().starts_with(&arg)
                    || insert.starts_with(&arg)
                {
                    let active = (insert == "en" && i18n::current() == Language::English)
                        || (insert == "zh" && i18n::current() == Language::Chinese);
                    // ThemeCommand::suggest_args: native name plus active suffix.
                    let description = format!(
                        "{display}{}",
                        if active {
                            i18n::label(" (active)", "（当前）")
                        } else {
                            ""
                        }
                    );
                    self.snapshot
                        .matches
                        .push(row(display, &description, insert));
                }
            }
        } else if query.is_empty() || command_prefix_matches_smart("language", query) {
            self.snapshot.matches.push(row(
                "/language",
                i18n::label("Change interface language", "切换界面语言"),
                "/language ",
            ));
            if let Some(row) = self.snapshot.matches.last_mut() {
                row.indices = (1..=query.chars().count() as u32).collect();
            }
        } else {
            return;
        }
        if changed && self.argument && query[9..].trim().is_empty() {
            let preferred = if i18n::current() == Language::Chinese {
                "zh"
            } else {
                "en"
            };
            self.snapshot.selected = self
                .snapshot
                .matches
                .iter()
                .position(|row| row.insert_text == preferred)
                .unwrap_or(0);
        }
        self.snapshot.open = true;
        self.snapshot.selected = self
            .snapshot
            .selected
            .min(self.snapshot.matches.len().saturating_sub(1));
    }
    pub fn dismiss(&mut self) {
        self.dismissed = Some(self.last.clone());
        self.snapshot.open = false;
        self.area.set(Rect::default());
        self.panel.set(Rect::default());
    }
    /// Original SlashController wraps Up/Down (also Ctrl-P/N) at both ends.
    pub fn move_selection(&mut self, delta: isize) {
        let n = self.snapshot.matches.len();
        if n > 0 {
            self.snapshot.selected =
                (self.snapshot.selected as isize + delta).rem_euclid(n as isize) as usize;
        }
    }
    /// Original SlashController::scroll_selection clamps wheel/page movement.
    pub fn scroll_selection(&mut self, delta: isize) {
        let n = self.snapshot.matches.len();
        if n > 0 {
            let current = self.snapshot.selected.min(n - 1) as isize;
            self.snapshot.selected = (current + delta).clamp(0, n as isize - 1) as usize;
        }
    }
    /// PromptWidget completion replaces the selected command/argument without
    /// dispatching it. Only Enter commits language.
    pub fn completion(&self) -> Option<String> {
        let row = self.snapshot.matches.get(self.snapshot.selected)?;
        Some(if self.argument {
            format!("/language {}", row.insert_text)
        } else {
            row.insert_text.clone()
        })
    }
    pub fn choice(&self) -> Option<Choice> {
        let item = self.snapshot.matches.get(self.snapshot.selected)?;
        Some(if self.argument {
            Choice::Language(if item.insert_text == "zh" {
                Language::Chinese
            } else {
                Language::English
            })
        } else {
            Choice::Arguments
        })
    }
    /// PromptWidget's slash-range paint and inline-ghost path. It uses the
    /// existing textarea layout, and never moves or replaces the real caret.
    pub fn draw_prompt_hint(
        &self,
        buf: &mut Buffer,
        textarea: &xai_ratatui_textarea::TextArea,
        state: xai_ratatui_textarea::TextAreaState,
        area: Rect,
    ) {
        if !self.open() {
            return;
        }
        let theme = Theme::current();
        let end = self
            .last
            .find(char::is_whitespace)
            .unwrap_or(self.last.len());
        paint_slash_token_highlight(textarea, state, area, buf, 0..end, theme.fuzzy_accent);
        if !self.argument
            && let Some(ghost) =
                inline_ghost_suffix(self.last.strip_prefix('/').unwrap_or(""), "language")
            && let Some((x, y)) = textarea.screen_position_of(end, area, state)
        {
            let avail = area.right().saturating_sub(x) as usize;
            if avail > 0 {
                let text = crate::render::line_utils::truncate_str(&ghost, avail);
                buf.set_string(
                    x,
                    y,
                    &text,
                    Style::default().fg(theme.gray).bg(theme.bg_base),
                );
            }
        }
    }
    pub fn draw(&self, buf: &mut Buffer, outer: Rect, prompt: Rect) {
        self.area.set(Rect::default());
        self.panel.set(Rect::default());
        *self.hit.borrow_mut() = Default::default();
        if !self.open() {
            return;
        }
        let items_width = crate::dropdown_chrome::dropdown_items_width(prompt);
        let rows = slash_dropdown::desired_item_rows(&self.snapshot.matches, items_width);
        if rows == 0 {
            return;
        }
        let theme = Theme::current();
        let Some(chrome) = crate::dropdown_chrome::render_dropdown_chrome(
            buf,
            self.snapshot.matches.len(),
            rows,
            None,
            prompt,
            outer,
            prompt.x.saturating_sub(outer.x),
            outer.right().saturating_sub(prompt.right()),
            false,
            &theme,
        ) else {
            return;
        };
        self.area.set(chrome.items);
        self.panel.set(chrome.panel);
        *self.hit.borrow_mut() =
            slash_dropdown::render_dropdown(buf, chrome.items, &self.snapshot, self.hover, &theme);
    }
    fn index(&self, x: u16, y: u16) -> Option<usize> {
        let area = self.area.get();
        if !area.contains(Position::new(x, y)) {
            return None;
        }
        let hit = self.hit.borrow();
        if hit.has_scrollbar && x >= area.right().saturating_sub(2) {
            return None;
        }
        hit.row_items.get((y - area.y) as usize).copied()
    }
    pub fn hover(&mut self, x: u16, y: u16) -> bool {
        self.hover = self.index(x, y);
        self.panel.get().contains(Position::new(x, y))
    }
    pub fn pointer(&mut self, x: u16, y: u16) -> bool {
        if let Some(i) = self.index(x, y) {
            self.snapshot.selected = i;
            true
        } else {
            false
        }
    }
}

// Original PromptWidget paint helper, Apache-2.0, fixed 2bdd1d6.
fn paint_slash_token_highlight(
    textarea: &xai_ratatui_textarea::TextArea,
    state: xai_ratatui_textarea::TextAreaState,
    ta_area: Rect,
    buf: &mut Buffer,
    range: std::ops::Range<usize>,
    fg: ratatui::style::Color,
) {
    // One row-span per visual row, from the textarea's own wrap ranges (the source of truth for where it broke the text)
    // Invalid ranges yield no spans
    for span in textarea.screen_spans_of_range(range, ta_area, state) {
        for x in span.left()..span.right() {
            if let Some(cell) = buf.cell_mut((x, span.y))
                && !cell.symbol().trim().is_empty()
            {
                cell.set_fg(fg);
            }
        }
    }
}

// Original SlashController smart-prefix and ghost-suffix algorithm; Dot needs
// only the suffix text because its sole command starts at byte zero.
fn command_prefix_matches_smart(full_name: &str, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    if query.chars().any(|c| c.is_uppercase()) {
        return full_name.starts_with(query);
    }
    let mut full_chars = full_name.chars();
    for qc in query.chars() {
        match full_chars.next() {
            Some(fc) if fc == qc || fc.eq_ignore_ascii_case(&qc) => {}
            _ => return false,
        }
    }
    true
}
fn inline_ghost_suffix(query: &str, full_name: &str) -> Option<String> {
    if query.is_empty() || !command_prefix_matches_smart(full_name, query) {
        return None;
    }
    let mut rest = full_name.chars();
    for _ in query.chars() {
        rest.next()?;
    }
    let text: String = rest.collect();
    if text.is_empty() { None } else { Some(text) }
}
