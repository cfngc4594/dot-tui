//! User-prompt fold policy from Grok Build 2bdd1d6, blocks/user.rs and
//! app/agent_view/selection.rs. xAI, Apache-2.0. Dot carries only text user
//! messages; tools/groups/assistant folding and remote settings are excluded.
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;
pub const COLLAPSED_MAX_LINES: usize = 3;
const MULTI_CLICK_TIMEOUT_MS: u128 = 300;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DisplayMode {
    Collapsed,
    Expanded,
    Truncated,
}
struct UserPromptBlock<'a> {
    text: &'a str,
}
impl UserPromptBlock<'_> {
    // Original is_foldable/default_display_mode/next_fold_mode bodies.
    fn is_foldable(&self) -> bool {
        const MIN_CONTENT_WIDTH: usize = 60;
        let mut visual_lines = 0usize;
        for line in self.text.lines() {
            let w = line.width();
            visual_lines += if w == 0 {
                1
            } else {
                w.div_ceil(MIN_CONTENT_WIDTH)
            };
            if visual_lines > COLLAPSED_MAX_LINES {
                return true;
            }
        }
        false
    }
    fn default_display_mode(&self) -> DisplayMode {
        if self.is_foldable() {
            DisplayMode::Collapsed
        } else {
            DisplayMode::Expanded
        }
    }
    fn next_fold_mode(&self, current: DisplayMode, _is_running: bool) -> DisplayMode {
        match current {
            DisplayMode::Collapsed | DisplayMode::Truncated => DisplayMode::Expanded,
            DisplayMode::Expanded => DisplayMode::Collapsed,
        }
    }
}
pub fn is_foldable(text: &str) -> bool {
    UserPromptBlock { text }.is_foldable()
}
pub fn expanded(text: &str, saved: Option<bool>) -> bool {
    saved.unwrap_or(UserPromptBlock { text }.default_display_mode() == DisplayMode::Expanded)
}
pub fn toggle(text: &str, current: bool) -> bool {
    UserPromptBlock { text }.next_fold_mode(
        if current {
            DisplayMode::Expanded
        } else {
            DisplayMode::Collapsed
        },
        false,
    ) == DisplayMode::Expanded
}
/// Single-key entry identity and 300ms window from handle_scrollback_click.
/// No x/y tolerance is added: fold multi-click counts by entry; the separate
/// original drag rule cancels clicks after one column OR row of movement.
pub fn click_count(
    last: Option<(std::time::Instant, &str, u8)>,
    now: std::time::Instant,
    id: &str,
) -> u8 {
    if let Some((last_time, last_id, prev_count)) = last
        && last_id == id
        && now.duration_since(last_time).as_millis() < MULTI_CLICK_TIMEOUT_MS
    {
        prev_count + 1
    } else {
        1
    }
}
pub fn drag_threshold_exceeded(start: (u16, u16), col: u16, row: u16) -> bool {
    let dx = start.0.abs_diff(col);
    let dy = start.1.abs_diff(row);
    dx >= 1 || dy >= 1
}
// Grok PromptWidget::chip_line and UserPromptBlock::wrap_prompt_lines (2bdd1d6).
// Dot adaptation: recognize canonical image placeholders in remote human text;
// this styles text only and does not bind a placeholder to a downloadable file.
fn image_chip_line(text: &str) -> Line<'static> {
    use ratatui::text::Span;
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[Image #") {
        let candidate = &rest[start..];
        let Some(end) = candidate.find(']') else {
            break;
        };
        let token = &candidate[..=end];
        if super::image_composer::parse_image_display_number(token).is_some() {
            spans.push(Span::raw(rest[..start].to_owned()));
            spans.extend(
                super::image_composer::chip_line(token[1..token.len() - 1].to_owned()).spans,
            );
        } else {
            spans.push(Span::raw(rest[..start + end + 1].to_owned()));
        }
        rest = &candidate[end + 1..];
    }
    spans.push(Span::raw(rest.to_owned()));
    Line::from(spans)
}
pub fn lines(text: &str, width: usize, prefix: &str, is_expanded: bool) -> Vec<Line<'static>> {
    let styled = text.split('\n').map(image_chip_line).collect();
    let mut content = super::grok_markdown::wrap_styled(styled, width.max(1));
    if !is_expanded && content.len() > COLLAPSED_MAX_LINES {
        content.truncate(COLLAPSED_MAX_LINES);
        let last = content.pop().unwrap();
        // Original keeps spans when rewrapping the tail for the ellipsis.
        let mut row = super::grok_markdown::wrap_styled(vec![last], width.saturating_sub(2).max(1))
            .into_iter()
            .next()
            .unwrap_or_default();
        row.spans.push(ratatui::text::Span::raw(" …"));
        content.push(row);
    }
    super::ui::user_prefix_lines(content, prefix)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upstream_fold_modes() {
        assert!(!is_foldable("1\n2\n3"));
        assert!(is_foldable("1\n2\n3\n4"));
        assert!(!is_foldable(&"中".repeat(90)));
        assert!(is_foldable(&"中".repeat(91)));
        assert!(!expanded("1\n2\n3\n4", None));
        assert!(expanded("short", None));
        assert_eq!(
            UserPromptBlock { text: "x" }.next_fold_mode(DisplayMode::Truncated, false),
            DisplayMode::Expanded
        );
    }
}
