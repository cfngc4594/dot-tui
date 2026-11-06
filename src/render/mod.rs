//! Original image overlay and bounds-checked primitives; fixed RGB theme.
#[allow(dead_code)]
pub mod image_overlay;
pub mod image_viewer;
#[allow(dead_code)]
pub mod line_utils;
#[allow(dead_code)]
pub mod safe_buf;
pub use safe_buf::SafeBuf;
pub mod color {
    use ratatui::{buffer::Buffer, layout::Rect, style::Color};
    // RGB path of original render/color.rs dim_area/blend_area. Dot has no Reset theme.
    pub fn dim_area(buf: &mut Buffer, area: Rect, base: Color, opacity: f32) {
        if let Color::Rgb(br, bg, bb) = base {
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    if let Some(c) = buf.cell_mut((x, y)) {
                        let blend = |base: u8, fg: u8| {
                            (base as f32 * (1.0 - opacity) + fg as f32 * opacity).round() as u8
                        };
                        if let Color::Rgb(r, g, b) = c.fg {
                            c.set_fg(Color::Rgb(blend(br, r), blend(bg, g), blend(bb, b)));
                        }
                        if let Color::Rgb(r, g, b) = c.bg {
                            c.set_bg(Color::Rgb(blend(br, r), blend(bg, g), blend(bb, b)));
                        }
                    }
                }
            }
        }
    }
}
