// Portions adapted from Grok Build, Copyright 2023-2026 SpaceXAI.
// SPDX-License-Identifier: Apache-2.0
// Upstream commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8
// Sources (crates/codegen/xai-grok-pager/src):
// - views/agent.rs: AgentViewLayout::compute, fill_background
// - views/prompt_widget/mod.rs: desired_height, content_width, draw divider loops
// - scrollback/blocks/user.rs: wrap_prompt_lines prefix/continuation construction
// - scrollback/wrappers/entry_renderer.rs: timestamp_reserved and content gutter
// Modifications: replace application/theme/config dependencies with explicit
// parameters; remove agent task panes, model picker, selection/collapse and tool
// controls; retain the original layout order, padding, prompt sizing formula,
// divider painting and prompt prefix/continuation algorithm. Dot-specific safe
// confirmation stays in ui.rs. No upstream backend/runtime is included.
use ratatui::{
    prelude::*,
    widgets::{Block, Padding, Widget},
};

pub const BASE: Color = Color::Rgb(18, 18, 18);
pub const BAND: Color = Color::Rgb(35, 35, 35);
pub const TEXT: Color = Color::Rgb(190, 190, 190);
pub const MUTED: Color = Color::Rgb(105, 105, 105);
pub const BORDER: Color = Color::Rgb(48, 48, 48);

pub struct AgentLayout {
    pub top: Rect,
    pub body: Rect,
    pub status: Rect,
    pub prompt: Rect,
    pub footer: Rect,
}
/// Adapted AgentViewLayout::compute: StatusBar, gap, scrollback, turn status,
/// prompt gap, measured prompt, shortcuts gap and shortcuts, in upstream order.
pub fn compute(area: Rect, text_height: u16) -> AgentLayout {
    let inner_area = Block::default()
        .padding(Padding::new(2, 2, 1, 1))
        .inner(area);
    let prompt_height = desired_height(text_height, area.height.saturating_sub(10).clamp(3, 8));
    let chunks = Layout::vertical([
        Constraint::Length(1), // StatusBar
        Constraint::Length(1), // status_gap
        Constraint::Min(1),    // scrollback
        Constraint::Length(1), // turn_status gap
        Constraint::Length(1), // turn_status
        Constraint::Length(1), // prompt_gap
        Constraint::Length(prompt_height),
        Constraint::Length(1), // shortcuts_gap
        Constraint::Length(1), // shortcuts
    ])
    .split(inner_area);
    AgentLayout {
        top: chunks[0],
        body: chunks[2],
        status: chunks[4],
        prompt: chunks[6],
        footer: chunks[8],
    }
}
/// Fill the screen area with base background and outer padding.
/// Adapted from views/agent.rs::fill_background; fixed dot theme/padding.
pub fn fill_background(buf: &mut Buffer, area: Rect) {
    buf.set_style(area, Style::default().bg(BASE));
    let outer_styled = Block::default()
        .padding(Padding::new(2, 2, 1, 1))
        .style(Style::default().bg(BASE));
    outer_styled.render(area, buf);
}
/// Compute the content width inside the chrome (if any).
/// Upstream PromptWidget::content_width with chrome=true, accent=1, pads=2/1.
pub fn content_width(area_width: u16) -> u16 {
    area_width.saturating_sub(1 + 2 + 1)
}
/// Height = vpad_top + textarea rows + info line. Clamped to max_height.
/// Adapted PromptWidget::desired_height. Dot has no history-browser freeze or
/// model flags; its info block is the bottom border, one row.
pub fn desired_height(text_height: u16, max_height: u16) -> u16 {
    let text_height = text_height.max(1);
    let vpad_top = 1;
    let info_block = 1;
    let total = vpad_top + text_height + info_block;
    let min = vpad_top + 1 + info_block;
    total.clamp(min.min(max_height), max_height)
}
/// Adapted verbatim divider/side painting algorithm from PromptWidget::draw.
/// Theme and viewport extracted into arguments, caption overlays handled by caller.
pub fn prompt_chrome(buf: &mut Buffer, area: Rect) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let div_style = Style::default().fg(BORDER).bg(BASE);
    let left_x = area.x;
    let right_x = area.x + area.width.saturating_sub(1);
    // Top divider: ╭──────────╮
    let div_y = area.y;
    for x in area.x..area.x + area.width {
        if let Some(cell) = buf.cell_mut((x, div_y)) {
            let ch = if x == left_x {
                '\u{256d}'
            } else if x == right_x {
                '\u{256e}'
            } else {
                '\u{2500}'
            };
            cell.set_char(ch);
            cell.set_style(div_style);
        }
    }
    // Side borders: │ on left and right of each text row.
    for y in area.y + 1..area.bottom() - 1 {
        if let Some(cell) = buf.cell_mut((left_x, y)) {
            cell.set_char('\u{2502}');
            cell.set_style(div_style);
        }
        if let Some(cell) = buf.cell_mut((right_x, y)) {
            cell.set_char('\u{2502}');
            cell.set_style(div_style);
        }
    }
    // Bottom divider: ╰──────────grok-3 · flags──╯
    // Modification: dot-specific caption, not a fabricated model name.
    let div_y = area.bottom() - 1;
    for x in area.x..area.x + area.width {
        if let Some(cell) = buf.cell_mut((x, div_y)) {
            let ch = if x == left_x {
                '\u{2570}'
            } else if x == right_x {
                '\u{256f}'
            } else {
                '\u{2500}'
            };
            cell.set_char(ch);
            cell.set_style(div_style);
        }
    }
}
/// Width reserved for the timestamp on the right side of content lines.
/// When non-zero, content is wrapped at `content_width - reserved` so text never
/// collides with the timestamp overlay. From EntryRenderer::timestamp_reserved.
pub fn timestamp_reserved(show: bool) -> u16 {
    if show {
        10
    } else {
        0
    }
}
/// Adapted wrap_prompt_lines prefix/continuation loop. Wrapping is injected so
/// both our Markdown and plaintext paths use the same bounded display cells.
pub fn user_lines(wrapped: Vec<Line<'static>>, prefix: &str) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthStr;
    let prefix_width = prefix.width();
    let prefix_style = Style::default().fg(TEXT);
    let mut all_lines = Vec::new();
    for (wrap_idx, wrapped_line) in wrapped.into_iter().enumerate() {
        let is_first_line = wrap_idx == 0;
        let indent: String = " ".repeat(prefix_width);
        let line_prefix = if is_first_line { prefix } else { &indent };
        let mut spans = vec![Span::styled(
            line_prefix.to_string(),
            prefix_style.patch(wrapped_line.style),
        )];
        spans.extend(wrapped_line.spans.into_iter().map(|s| Span {
            content: s.content.to_string().into(),
            style: wrapped_line.style.patch(s.style),
        }));
        all_lines.push(Line::from(spans));
    }
    all_lines
}

/// Original fancy frame sequence from pager-render/src/glyphs.rs::braille_spinner_frames.
/// Cadence from views/turn_status.rs::SPINNER_DIVISOR (4 ticks at ~30 fps).
/// This Unix-first client omits upstream legacy Windows console detection.
pub fn activity_frame(elapsed_ms: u128) -> &'static str {
    const FRAMES: &[&str] = &[
        "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}",
        "\u{2827}",
    ];
    FRAMES[((elapsed_ms / 133) as usize) % FRAMES.len()]
}
