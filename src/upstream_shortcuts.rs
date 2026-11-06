//! Extracted from Grok Build shortcuts_bar.rs, fixed 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
//! Copyright xAI. Apache-2.0; see THIRD_PARTY_NOTICES.md.
//! Dot adaptation: HintItem contains a single custom-display key and label;
//! no registry/keybinding/theme runtime is imported. These hints have no paired
//! keys, so bar_key_segments uses the upstream single-key custom-display path.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

struct HintItem<'a> {
    key: &'a str,
    label: &'a str,
}
struct BarKeySeg {
    text: String,
    is_join: bool,
}
fn bar_key_segments(hint: &HintItem<'_>) -> Vec<BarKeySeg> {
    let text = hint.key.to_string();
    if text.is_empty() {
        return Vec::new();
    }
    vec![BarKeySeg {
        text,
        is_join: false,
    }]
}
// Original paint_key_run and paint_hint_keys bodies retained verbatim.
fn paint_key_run(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) -> u16 {
    if text.is_empty() {
        return x;
    }
    let w = text.width() as u16;
    let span = Span::styled(text, style);
    buf.set_span(x, y, &span, w);
    x + w
}
fn paint_hint_keys(
    buf: &mut Buffer,
    mut x: u16,
    y: u16,
    hint: &HintItem,
    key_style: Style,
    sep_style: Style,
) -> u16 {
    for seg in bar_key_segments(hint) {
        let style = if seg.is_join { sep_style } else { key_style };
        x = paint_key_run(buf, x, y, &seg.text, style);
    }
    x
}
// Actual GrokNight fields: bg_base=BG_STORM, text_secondary=FG_DARK,
// gray=COMMENT from pager-render/src/theme/groknight.rs. Theme::muted()
// returns explicit gray without DIM for RGB themes (tokyonight.rs).
fn key_style() -> Style {
    Style::default()
        .fg(Color::Rgb(200, 200, 200))
        .bg(Color::Rgb(20, 20, 20))
        .add_modifier(Modifier::BOLD)
}
fn action_style() -> Style {
    Style::default()
        .fg(Color::Rgb(108, 108, 108))
        .bg(Color::Rgb(20, 20, 20))
        .remove_modifier(Modifier::BOLD)
}
pub(super) fn separator_style() -> Style {
    action_style().add_modifier(Modifier::DIM)
}
pub(super) fn render_hint(buf: &mut Buffer, area: Rect, text: &str) {
    let (key, label) = text.split_once(':').unwrap_or(("", text));
    let hint = HintItem { key, label };
    let mut x = paint_hint_keys(buf, area.x, area.y, &hint, key_style(), action_style());
    // Dot action-only mouse controls have no key or colon.
    if !key.is_empty() {
        buf.set_span(x, area.y, &Span::styled(":", action_style()), 1);
        x += 1;
    }
    buf.set_span(
        x,
        area.y,
        &Span::styled(hint.label, action_style()),
        hint.label.width() as u16,
    );
}

// Original row clear prevents modifiers/text leaking from an earlier frame.
pub(super) fn clear_row(buf: &mut Buffer, area: Rect) {
    let bg_style = action_style().remove_modifier(Modifier::all());
    for x in area.x..area.x + area.width {
        if let Some(cell) = buf.cell_mut((x, area.y)) {
            cell.reset();
            cell.set_style(bg_style);
        }
    }
}
