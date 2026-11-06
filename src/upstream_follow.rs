//! Grok 2bdd1d6 app/agent_view/render.rs::draw_scroll_arrow, Apache-2.0.
//! Dot places the original one-row ▼/three-cell hit target in its status strip.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
pub fn hit(area: Rect) -> Option<Rect> {
    if area.width < 3 || area.height == 0 {
        return None;
    }
    let center = area.x + area.width / 2;
    Some(Rect::new(center.saturating_sub(1), area.y, 3, 1))
}
pub fn draw(buf: &mut Buffer, area: Rect, hovered: bool) -> Option<Rect> {
    let hit = hit(area)?;
    let center_x = area.x + area.width / 2;
    let style = Style::default().fg(if hovered {
        Color::Rgb(120, 120, 120)
    } else {
        Color::Rgb(108, 108, 108)
    });
    if let Some(cell) = buf.cell_mut((center_x, area.y)) {
        cell.set_symbol("▼");
        cell.set_style(style);
    }
    Some(hit)
}
