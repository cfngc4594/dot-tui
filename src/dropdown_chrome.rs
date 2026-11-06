// Adapted from Grok Build 2bdd1d6 app/agent_view/mod.rs, Apache-2.0.
// Copyright 2023–2026 SpaceXAI. Dot passes its existing layout padding instead
// of Grok LayoutConfig; fixed RGB/fullscreen chrome omits embedded-only branch.
use crate::{render::SafeBuf, theme::Theme};
use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Line, widgets::Widget};

/// Render dropdown chrome (borders, count hint) anchored to the prompt and return the inner items area. Returns `None` when geometry doesn't fit.
/// `below = false` anchors the panel *above* the prompt (full-TUI default); `below = true` anchors it *below* the prompt (minimal mode, common CLI style, to reduce layout shift).
/// Shared by slash dropdown and completion dropdown to avoid duplicated chrome code.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_dropdown_chrome(
    buf: &mut Buffer,
    item_count: usize,
    item_rows: u16,
    inline_prompt_area: Option<Rect>,
    layout_prompt: Rect,
    area: Rect,
    hpad_left: u16,
    hpad_right: u16,
    below: bool,
    theme: &Theme,
) -> Option<DropdownChrome> {
    let mut panel_height = item_rows + 2;
    let (top_border_y, bottom_border_y) = if below {
        let anchor = inline_prompt_area.unwrap_or(layout_prompt);
        let top = anchor.y + anchor.height;
        (top, top + panel_height - 1)
    } else {
        let bottom = if let Some(ipa) = inline_prompt_area {
            ipa.y.saturating_sub(1)
        } else {
            layout_prompt.y.saturating_sub(1)
        };
        let avail = bottom.saturating_sub(area.y).saturating_add(1);
        panel_height = panel_height.min(avail);
        if panel_height < 3 {
            return None;
        }
        (bottom.saturating_sub(panel_height - 1), bottom)
    };
    let panel_x = area.x + hpad_left;
    let panel_width = area.width.saturating_sub(hpad_left + hpad_right);
    if top_border_y >= bottom_border_y || panel_width <= 4 {
        return None;
    }
    if below && bottom_border_y > area.y + area.height.saturating_sub(1) {
        return None;
    }
    let panel_area = Rect {
        x: panel_x,
        y: top_border_y,
        width: panel_width,
        height: panel_height,
    };
    if panel_area.bottom() > buf.area.bottom()
        || panel_area.right() > buf.area.right()
        || panel_area.y < buf.area.y
    {
        return None;
    }
    ratatui::widgets::Clear.render(panel_area, buf);
    {
        buf.set_style(
            panel_area,
            Style::default().fg(theme.text_primary).bg(theme.bg_light),
        );
        let border_style = Style::default()
            .fg(theme.panel_border_fg())
            .bg(theme.bg_base);
        let border_line = Line::styled("\u{2500}".repeat(panel_width as usize), border_style);
        buf.set_line_safe(panel_x, top_border_y, &border_line, panel_width);
        buf.set_line_safe(panel_x, bottom_border_y, &border_line, panel_width);
        let hint = format!("{}", item_count);
        let hint_w = hint.len() as u16;
        if hint_w + 2 <= panel_width {
            let hint_x = panel_x + panel_width - hint_w - 1;
            let hint_line = Line::styled(hint, Style::default().fg(theme.gray).bg(theme.bg_base));
            buf.set_line_safe(hint_x, top_border_y, &hint_line, hint_w);
        }
    }
    let content_inset = dropdown_content_inset();
    let items_x = layout_prompt.x + content_inset;
    let items_width = layout_prompt.width.saturating_sub(content_inset);
    Some(DropdownChrome {
        items: Rect {
            x: items_x,
            y: top_border_y + 1,
            height: panel_height - 2,
            width: items_width,
        },
        panel: panel_area,
    })
}
/// Left inset of dropdown item rows inside the panel (see the comment in [`render_dropdown_chrome`]).
/// [`render_dropdown_chrome`]).
pub(crate) fn dropdown_content_inset() -> u16 {
    2
}
/// Width of the dropdown item rows [`render_dropdown_chrome`] will produce for `layout_prompt`, for sizing the row count *before* drawing the chrome.
/// `layout_prompt`, for sizing the row count *before* drawing the chrome.
pub(crate) fn dropdown_items_width(layout_prompt: Rect) -> u16 {
    layout_prompt.width.saturating_sub(dropdown_content_inset())
}
/// Geometry returned by [`render_dropdown_chrome`]: the inset `items` area for rendering rows and the full `panel` rect (borders + padding) used as an occluder for hyperlink hit-testing.
/// rendering rows and the full `panel` rect (borders + padding) used as an
/// occluder for hyperlink hit-testing.
pub(crate) struct DropdownChrome {
    pub(crate) items: Rect,
    pub(crate) panel: Rect,
}
