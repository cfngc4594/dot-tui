// Adapted from Grok Build, Copyright 2023–2026 SpaceXAI, Apache-2.0.
// Source: crates/codegen/xai-grok-pager-render/src/render/scrollbar.rs
// Commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8
// Production geometry, ScrollMetrics click inverse and tui-scrollbar rendering retained.
// Changes: fixed GrokNight styles with agent.rs follow blend; upstream tests omitted.
//! This module provides scrollbar rendering using `tui-scrollbar` for smooth Unicode-based scrollbars with sub-character precision.
//!
//! # Visual Design
//!
//! The scrollbar visibility indicates follow mode state:
//! - **Following (at bottom):** Very dim scrollbar (subtle indicator of content above)
//! - **Not following:** Brighter scrollbar (draws attention to "scrolled up" state)
//!
//! # Layout
//!
//! Callers should reserve space for the scrollbar:
//! - 1 column gap (visual separation from content)
//! - 1 column track (the scrollbar itself)
//!
//! Use [`split_area_for_scrollbar`] to compute content and scrollbar areas.
//!
//! # TODO: Mouse Support
//!
//! `tui-scrollbar` already provides mouse interaction support via:
//! - [`tui_scrollbar::ScrollBarInteraction`] for drag state
//! - [`tui_scrollbar::ScrollEvent`] / [`tui_scrollbar::PointerEvent`] for input
//! - [`tui_scrollbar::ScrollBar::handle_event`] for hit testing and drag math
//!
//! To wire this up:
//! 1. Store `ScrollBarInteraction` in pane state
//! 2. Translate crossterm `MouseEvent` to `tui_scrollbar::PointerEvent`
//! 3. Call `scrollbar.handle_event()` to get `ScrollCommand::SetOffset`
//! 4. Update scroll position accordingly

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui_core::buffer::Buffer as CoreBuffer;
use ratatui_core::layout::Rect as CoreRect;
use ratatui_core::widgets::Widget as _;
use tui_scrollbar::ScrollBar;
use tui_scrollbar::ScrollLengths;
use tui_scrollbar::{SUBCELL, ScrollMetrics};

/// When set, every scrollbar renders as a no-op.
/// The pager toggles this on in minimal (scrollback-native) mode, where lists/dropdowns show no scrollbar at all.
/// They scroll internally and the footer carries the "↑/↓ navigate" hint.
static SCROLLBARS_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Globally hide or show all scrollbars. See [`SCROLLBARS_HIDDEN`].
pub fn set_scrollbars_hidden(hidden: bool) {
    SCROLLBARS_HIDDEN.store(hidden, Ordering::Relaxed);
}

/// Whether scrollbars are currently globally hidden.
pub fn scrollbars_hidden() -> bool {
    SCROLLBARS_HIDDEN.load(Ordering::Relaxed)
}

/// Number of columns reserved between content and the scrollbar track.
const SCROLLBAR_GAP_COLS: u16 = 1;

/// Width of the scrollbar track itself (in terminal cells).
const SCROLLBAR_TRACK_COLS: u16 = 1;

/// Total columns reserved for scrollbar UI (gap + track).
pub const SCROLLBAR_TOTAL_COLS: u16 = SCROLLBAR_GAP_COLS + SCROLLBAR_TRACK_COLS;

/// Always reserves the track plus a blank gap. `(content_area, None)` when too narrow.
/// Use [`maybe_split_for_scrollbar`] to reserve only when the bar will be shown.
pub fn split_area_for_scrollbar(area: Rect) -> (Rect, Option<Rect>) {
    if area.width <= SCROLLBAR_TOTAL_COLS {
        return (area, None);
    }

    let content_width = area.width.saturating_sub(SCROLLBAR_TOTAL_COLS);
    let content_area = Rect {
        x: area.x,
        y: area.y,
        width: content_width,
        height: area.height,
    };
    let scrollbar_area = Rect {
        x: area.right().saturating_sub(1),
        y: area.y,
        width: SCROLLBAR_TRACK_COLS,
        height: area.height,
    };

    (content_area, Some(scrollbar_area))
}

/// Unlike [`split_area_for_scrollbar`], this gives full width to content when scrollbar won't be shown (`total_lines <= viewport_lines`).
///
/// Use this when you know the content height before splitting.
pub fn maybe_split_for_scrollbar(area: Rect, total_lines: usize) -> (Rect, Option<Rect>) {
    if needs_scrollbar(total_lines, area.height) {
        split_area_for_scrollbar(area)
    } else {
        (area, None)
    }
}

/// Whether the scrollbar should be shown (content overflows viewport).
pub fn needs_scrollbar(total_lines: usize, viewport_lines: u16) -> bool {
    total_lines > viewport_lines as usize
}

/// Track plus one column of slop: a thumb flush against a border is read as a two-column widget, so near-miss presses must still grab.
pub fn scrollbar_grab_zone(track: Rect) -> Rect {
    let x = track.x.saturating_sub(SCROLLBAR_GAP_COLS);
    Rect {
        x,
        y: track.y,
        width: (track.x - x).saturating_add(track.width).saturating_add(1),
        height: track.height,
    }
}

/// Result of mapping a scrollbar click/drag position to a scroll offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarClickResult {
    /// Jump to the very top (click on first row of track).
    Top,
    /// Jump to the very bottom (click on last row of track).
    Bottom,
    /// Set scroll offset to this value (proportional position).
    Offset(usize),
}

/// Inverse of the renderer's `ScrollMetrics`, so a click lands where the thumb is drawn.
/// First/last row is Top/Bottom; otherwise the thumb is centered on the click.
pub fn scrollbar_click_to_offset(
    cell_index: u16,
    track_cells: u16,
    total_lines: usize,
    viewport_lines: u16,
) -> ScrollbarClickResult {
    if track_cells == 0 {
        return ScrollbarClickResult::Top;
    }

    if cell_index == 0 {
        return ScrollbarClickResult::Top;
    }
    if cell_index >= track_cells.saturating_sub(1) {
        return ScrollbarClickResult::Bottom;
    }

    let lengths = ScrollLengths {
        content_len: total_lines as usize,
        viewport_len: viewport_lines as usize,
    };
    let metrics = ScrollMetrics::new(lengths, 0, track_cells);

    // Center the thumb on the clicked cell (same as tui_scrollbar JumpToClick).
    let position = (cell_index as usize)
        .saturating_mul(SUBCELL)
        .saturating_add(SUBCELL / 2);
    let half_thumb = metrics.thumb_len() / 2;
    let thumb_start = position.saturating_sub(half_thumb);
    let offset = metrics.offset_for_thumb_start(thumb_start);

    ScrollbarClickResult::Offset(offset)
}

/// Always drawn on overflow. Dim while following; brighter when detached so the user notices they left the tail.
pub fn render_scrollbar(
    buf: &mut Buffer,
    scrollbar_area: Option<Rect>,
    total_lines: usize,
    viewport_lines: u16,
    offset: usize,
    is_following: bool,
) {
    let (track_style, thumb_style) = scrollbar_styles(is_following);
    render_scrollbar_styled(
        buf,
        scrollbar_area,
        total_lines,
        viewport_lines,
        offset,
        track_style,
        thumb_style,
    );
}

/// Some emulators (notably macOS Terminal.app) do not stretch the `█` glyph over the cell's line-gap pixels.
/// A foreground-only thumb therefore renders striped with dark bars; the background fill covers the whole cell box.
fn thumb_fill_style(thumb_style: Style) -> Style {
    match thumb_style.fg {
        Some(fg) => thumb_style.bg(fg),
        None => thumb_style,
    }
}

/// Get track and thumb styles based on follow mode.
fn scrollbar_styles(is_following: bool) -> (Style, Style) {
    // GrokNight BG_STORM_DARK / BG_HIGHLIGHT and agent.rs's 0.4 follow blend.
    // Fixed theme replaces the original application's Theme::current().
    use ratatui::style::Color;
    let bg = Color::Rgb(17, 17, 17);
    let fg = if is_following {
        Color::Rgb(25, 25, 25)
    } else {
        Color::Rgb(36, 36, 36)
    };
    (Style::default().bg(bg), Style::default().fg(fg).bg(bg))
}

/// Like [`render_scrollbar`] but allows custom styling for theme integration.
pub fn render_scrollbar_styled(
    buf: &mut Buffer,
    scrollbar_area: Option<Rect>,
    total_lines: usize,
    viewport_lines: u16,
    offset: usize,
    track_style: Style,
    thumb_style: Style,
) {
    if SCROLLBARS_HIDDEN.load(Ordering::Relaxed) {
        return;
    }

    let Some(scrollbar_area) = scrollbar_area else {
        return;
    };

    if scrollbar_area.width == 0 || scrollbar_area.height == 0 {
        return;
    }

    if !needs_scrollbar(total_lines, viewport_lines) {
        return;
    }

    let lengths = ScrollLengths {
        content_len: total_lines as usize,
        viewport_len: viewport_lines as usize,
    };

    let scrollbar = ScrollBar::vertical(lengths).offset(offset as usize);

    // Render into ratatui-core scratch buffer
    let core_area = CoreRect {
        x: scrollbar_area.x,
        y: scrollbar_area.y,
        width: scrollbar_area.width,
        height: scrollbar_area.height,
    };
    let mut scratch = CoreBuffer::empty(core_area);
    (&scrollbar).render(core_area, &mut scratch);

    // Copy to ratatui buffer with custom styling
    let thumb_fill = thumb_fill_style(thumb_style);
    for row in 0..scrollbar_area.height {
        let x = scrollbar_area.x;
        let y = scrollbar_area.y + row;
        let Some(src) = scratch.cell((x, y)) else {
            continue;
        };
        let Some(dst) = buf.cell_mut((x, y)) else {
            continue;
        };
        if src.symbol() == " " {
            dst.set_symbol(" ");
            dst.set_style(track_style);
        } else {
            dst.set_symbol("\u{2588}");
            dst.set_style(thumb_fill);
        }
    }
}
