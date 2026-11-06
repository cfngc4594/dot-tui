use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
pub fn floor_char_boundary(s: &str, index: usize) -> usize {
    let index = index.min(s.len());
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
pub fn truncate_str(s: &str, max_width: usize) -> String {
    crate::util::truncate_to_width(s, max_width).into_owned()
}
pub fn truncate_line(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 {
        return Line::from(vec![]);
    }

    let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
    if total <= max_width {
        return line;
    }

    // Need room for the ellipsis (1 column).
    let budget = max_width.saturating_sub(1);
    let mut used = 0usize;
    let mut out: Vec<Span<'static>> = Vec::new();

    for span in line.spans {
        let sw = span.content.width();
        if used + sw <= budget {
            // Entire span fits.
            used += sw;
            out.push(span);
        } else {
            // Partial fit: truncate this span
            let remaining = budget - used;
            if remaining > 0 {
                let truncated = take_width(&span.content, remaining);
                out.push(Span::styled(truncated, span.style));
            }
            // Append ellipsis with the same style as the last span.
            let ellipsis_style = out.last().map(|s| s.style).unwrap_or_default();
            out.push(Span::styled("\u{2026}", ellipsis_style));
            return Line::from(out);
        }
    }

    // Shouldn't reach here (total > max_width checked above), but be safe.
    Line::from(out)
}

fn take_width(s: &str, n: usize) -> String {
    let Some(prefix) = s.get(..crate::util::byte_offset_at_width(s, n)) else {
        return String::new();
    };
    prefix.to_owned()
}
