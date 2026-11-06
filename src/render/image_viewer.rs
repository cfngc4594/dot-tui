//! Adapted from Grok Build 2bdd1d6, agent_view/render.rs:3402–3567 and
//! views/modal_window.rs::render_close_button. Copyright 2023–2026 SpaceXAI,
//! Apache-2.0; see THIRD_PARTY_NOTICES.md and licenses/.
//! Dot supplies worker-prepared PastedImage instead of AgentView's local-path
//! viewer state. Its header remains outside the modal for the unified status.
use crate::{prompt_images::PastedImage, render::SafeBuf, terminal::overlay, theme::Theme};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

pub fn popup(area: Rect) -> Rect {
    let width = ((area.width as u32 * 90) / 100)
        .max(28)
        .min(area.width as u32) as u16;
    let height = ((area.height as u32 * 90) / 100)
        .max(8)
        .min(area.height as u32) as u16;
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
pub fn close_button(popup: Rect) -> Rect {
    Rect::new(popup.x + popup.width.saturating_sub(7), popup.y, 5, 1)
}
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    image: Option<&PastedImage>,
) -> Option<overlay::Escapes> {
    let theme = Theme::current();
    let popup_rect = popup(area);
    if popup_rect.width < 10 || popup_rect.height < 5 {
        return overlay::clear();
    }
    super::color::dim_area(buf, area, theme.bg_base, 0.5);
    Clear.render(popup_rect, buf);
    buf.set_style(
        popup_rect,
        Style::default().fg(theme.text_primary).bg(theme.bg_base),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.gray_dim))
        .style(Style::default().bg(theme.bg_base));
    let inner = block.inner(popup_rect);
    block.render(popup_rect, buf);
    let title_style = Style::default()
        .fg(theme.text_primary)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    let name = image
        .and_then(|i| i.source_path.as_ref())
        .and_then(|p| p.file_name())
        .map(|n| crate::sanitize::strip_control_chars(&n.to_string_lossy(), false))
        .unwrap_or_else(|| "Image".into());
    let dimensions = image.and_then(PastedImage::preview_dimensions);
    let dims = dimensions
        .map(|(w, h)| format!(" ({w}×{h})"))
        .unwrap_or_default();
    let title_spans = [
        Span::styled("─ ", dim_style),
        Span::styled(name, title_style),
        Span::styled(dims, dim_style),
        Span::styled(" ─", dim_style),
    ];
    // Original five-cell close control; Dot mouse routing shares this rectangle.
    let close = close_button(popup_rect);
    for (offset, sym) in [" ", "[", "✗", "]", " "].iter().enumerate() {
        if let Some(cell) = buf.cell_mut((close.x + offset as u16, close.y)) {
            cell.set_symbol(sym);
        }
    }
    let max_title_width = popup_rect.width.saturating_sub(2 + close.width + 1 + 1);
    let mut cursor = 0;
    for span in &title_spans {
        let width = span.content.width() as u16;
        if cursor + width > max_title_width {
            break;
        }
        buf.set_span_safe(popup_rect.x + 2 + cursor, popup_rect.y, span, width);
        cursor += width;
    }
    let escapes = image.and_then(|i| {
        i.preview.prepared().and_then(|(bytes, (w, h))| {
            overlay::static_centered(bytes, w, h, popup_rect, i.preview.identity())
        })
    });
    if escapes.is_none()
        && let Some(image) = image
    {
        let (w, h) = dimensions.unwrap_or((0, 0));
        Paragraph::new(vec![
            Line::from(""),
            Line::from(format!("  {w}x{h} {}", image.mime_type)),
            Line::from(""),
            Line::from("  Press Esc to close"),
        ])
        .style(dim_style)
        .render(inner, buf);
    }
    // Loading/error details are supplied by Dot's existing top-right status.
    // Grok's pixel path and unsupported-terminal metadata fallback stay intact.
    escapes.or_else(overlay::clear)
}
