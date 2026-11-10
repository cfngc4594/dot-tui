//! Extracted Grok Build copy-feedback presentation, commit 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
//! Sources: pager/src/app/agent_view/{notices,render}.rs and
//! pager-render/src/clipboard/mod.rs. Copyright 2023–2026 SpaceXAI, Apache-2.0.
//! Dot adaptations: elapsed time replaces 30fps ticks, fixed GrokNight colors,
//! confirmed local clipboard success/failure (no OSC52/file fallback), localized
//! labels, and grapheme/terminal-cell-aware fitting and painting.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyOutcome {
    Copied,
    Failed,
}
impl CopyOutcome {
    fn message(self) -> String {
        crate::i18n::ui(match self {
            Self::Copied => "Copied!",
            // Dot has no /doctor or /minimal; do not advertise Grok-only commands.
            Self::Failed => "Copy failed.",
        })
    }
    fn duration(self) -> Duration {
        // Original ClipboardFeedback::{Copied,Failed}.ticks(): 30 / 120 at 30fps.
        Duration::from_secs(match self {
            Self::Copied => 1,
            Self::Failed => 4,
        })
    }
}
#[derive(Default)]
pub(crate) struct CopyToast {
    toast: Option<(CopyOutcome, Instant)>,
}
impl CopyToast {
    pub(crate) fn show(&mut self, outcome: CopyOutcome, at: Instant) {
        self.toast = Some((outcome, at));
    }
    /// Keep a real copy outcome visible while a UI snapshot is prepared.
    /// Lifetime boundaries are verified separately with explicit Instants.
    #[cfg(test)]
    pub(crate) fn pin_for_snapshot(&mut self) {
        if let Some((_, at)) = &mut self.toast {
            *at = Instant::now() + Duration::from_secs(24 * 60 * 60);
        }
    }
    pub(crate) fn message(&self, now: Instant) -> Option<String> {
        let (outcome, at) = self.toast?;
        (now.saturating_duration_since(at) < outcome.duration()).then(|| outcome.message())
    }
}
/// Preserve the upstream padding/ellipsis policy, measured in terminal cells.
fn fit_toast_text(msg: &str, avail_width: u16) -> Option<String> {
    let max_msg_width = (avail_width as usize).saturating_sub(4);
    if max_msg_width == 0 {
        return None;
    }
    if msg.width() <= max_msg_width {
        return Some(format!(" {msg} "));
    }
    let mut remaining = max_msg_width.saturating_sub("…".width());
    let truncated: String = msg
        .graphemes(true)
        .take_while(|grapheme| {
            if let Some(next) = remaining.checked_sub(grapheme.width()) {
                remaining = next;
                true
            } else {
                false
            }
        })
        .collect();
    Some(format!(" {}… ", truncated.trim_end()))
}
/// Original bottom-right scrollback slot/painting, with explicit pane and RGB theme.
pub(crate) fn render(buf: &mut Buffer, sb: Rect, msg: &str) -> Option<Rect> {
    let sb = sb.intersection(buf.area);
    if sb.height == 0 {
        return None;
    }
    let toast_text = fit_toast_text(msg, sb.width)?;
    let w = toast_text.width() as u16;
    let x = sb.right().saturating_sub(w + 1);
    let y = sb.bottom().saturating_sub(1);
    let area = Rect::new(x, y, w, 1);
    // Ratatui preserves grapheme clusters and clears the continuation cells of
    // wide characters, rather than writing the next character over their halves.
    buf.set_stringn(x, y, toast_text, w as usize, Style::default());
    // Set the exact upstream style over every occupied cell, including wide
    // continuation cells, and discard any underlying selection/text modifiers.
    for col in area.x..area.right() {
        let cell = &mut buf[(col, y)];
        cell.fg = Color::Rgb(200, 200, 200); // GrokNight accent_user = FG_DARK
        cell.bg = Color::Rgb(20, 20, 20); // GrokNight bg_base
        cell.modifier = Modifier::BOLD;
    }
    Some(area)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_message_is_padded_untouched() {
        assert_eq!(fit_toast_text("Copied!", 40).as_deref(), Some(" Copied! "));
    }
    #[test]
    fn long_message_truncates_with_ellipsis_instead_of_vanishing() {
        let fitted = fit_toast_text("Copy failed. Clipboard service is unavailable.", 24).unwrap();
        assert!(fitted.width() <= 22);
        assert!(fitted.ends_with("… "));
    }
    #[test]
    fn zero_width_slot_yields_none() {
        assert_eq!(fit_toast_text("Copied!", 4), None);
        assert_eq!(fit_toast_text("Copied!", 0), None);
    }
    #[test]
    fn feedback_lifetime_and_latest_result_match_upstream_ticks() {
        let now = Instant::now();
        let mut toast = CopyToast::default();
        toast.show(CopyOutcome::Copied, now);
        assert_eq!(
            toast.message(now + Duration::from_millis(999)).as_deref(),
            Some("Copied!")
        );
        assert_eq!(toast.message(now + Duration::from_secs(1)), None);
        toast.show(CopyOutcome::Failed, now);
        assert_eq!(
            toast.message(now + Duration::from_secs(1)).as_deref(),
            Some("Copy failed.")
        );
        assert_eq!(toast.message(now + Duration::from_secs(4)), None);
    }
    #[test]
    fn original_slot_is_bold_at_bottom_right_without_touching_prompt() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 25));
        let pane = Rect::new(2, 3, 50, 15);
        let hit = render(&mut buf, pane, "Copied!").unwrap();
        assert_eq!(hit, Rect::new(42, 17, 9, 1));
        let text: String = (hit.x..hit.right())
            .map(|x| buf[(x, hit.y)].symbol())
            .collect();
        assert_eq!(text, " Copied! ");
        assert_eq!(buf[(43, 17)].fg, Color::Rgb(200, 200, 200));
        assert!(buf[(43, 17)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(43, 18)].symbol(), " ");
    }
}

#[cfg(test)]
mod unicode_tests {
    use super::*;

    #[test]
    fn fitting_counts_wide_cells_without_splitting_graphemes() {
        assert_eq!(
            fit_toast_text("已复制！", 12).as_deref(),
            Some(" 已复制！ ")
        );
        assert_eq!(fit_toast_text("已复制！", 11).as_deref(), Some(" 已复制… "));
        assert_eq!(fit_toast_text("复制失败。", 9).as_deref(), Some(" 复制… "));
        assert_eq!(fit_toast_text("复制失败。", 5).as_deref(), Some(" … "));
        assert_eq!(
            fit_toast_text("e\u{301}abcd", 7).as_deref(),
            Some(" e\u{301}a… ")
        );
        assert_eq!(fit_toast_text("👩‍💻abcd", 7).as_deref(), Some(" 👩‍💻… "));
        for text in [
            "已复制！",
            "复制失败。",
            "Copied!",
            "Copy failed.",
            "e\u{301}abcd",
            "👩‍💻abcd",
        ] {
            for width in 0..40 {
                if let Some(fitted) = fit_toast_text(text, width) {
                    assert!(fitted.width() <= width.saturating_sub(2) as usize);
                } else {
                    assert!(width <= 4);
                }
            }
        }
    }

    #[test]
    fn chinese_toast_uses_full_cells_and_clears_underlying_selection() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 25));
        for cell in &mut buf.content {
            cell.set_char('x');
            cell.modifier = Modifier::REVERSED | Modifier::UNDERLINED;
        }
        let before = buf.clone();
        let area = render(&mut buf, Rect::new(2, 3, 50, 15), "已复制！").unwrap();
        assert_eq!(area, Rect::new(41, 17, 10, 1));
        for (offset, symbol) in [" ", "已", " ", "复", " ", "制", " ", "！", " ", " "]
            .into_iter()
            .enumerate()
        {
            assert_eq!(buf[(area.x + offset as u16, area.y)].symbol(), symbol);
        }
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if area.contains((x, y).into()) {
                    assert_eq!(buf[(x, y)].fg, Color::Rgb(200, 200, 200));
                    assert_eq!(buf[(x, y)].bg, Color::Rgb(20, 20, 20));
                    assert_eq!(buf[(x, y)].modifier, Modifier::BOLD);
                } else {
                    assert_eq!(buf[(x, y)], before[(x, y)]);
                }
            }
        }
    }

    #[test]
    fn empty_and_partially_clipped_panes_do_not_paint_outside_buffer() {
        let mut buf = Buffer::empty(Rect::new(5, 6, 10, 2));
        assert_eq!(render(&mut buf, Rect::new(0, 0, 60, 0), "已复制！"), None);
        assert_eq!(render(&mut buf, Rect::new(30, 20, 10, 1), "已复制！"), None);
        assert_eq!(
            render(&mut buf, Rect::new(0, 0, 60, 25), "已复制！"),
            Some(Rect::new(7, 7, 7, 1))
        );
    }
}
