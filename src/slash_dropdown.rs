// Adapted from Grok Build 2bdd1d6, Copyright 2023–2026 SpaceXAI, Apache-2.0.
// Dot: fixed RGB theme, local single-command snapshot, local scrollbar signature.
//! Renders slash command/arg suggestions as a scrollable list, following the same layout as the question/answer panel:
//! - Aligned label column (truncated with `…` when too long)
//! - Description text after a fixed gap, truncated to remaining width
//! - Selection highlight (bg_visual and bold on the selected row)
//! - Mouse hover highlight (25% blended bg)
//! - Scrollbar when results exceed visible height

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::render::SafeBuf;
use crate::render::line_utils::truncate_str;
use crate::scrollbar::render_scrollbar_styled;
use crate::slash::{MAX_VISIBLE_SUGGESTIONS, SlashSnapshot, SuggestionRow};
use crate::theme::Theme;

/// Maximum number of visible rows in the dropdown (excluding separator).
pub const MAX_DROPDOWN_ROWS: u16 = MAX_VISIBLE_SUGGESTIONS as u16;

/// Hard cap on label column width (labels longer than this are truncated).
const LABEL_CAP: usize = 40;

/// Gap (in spaces) between the label column and the description.
const LABEL_DESC_GAP: usize = 2;

/// Prefix display width in columns (`"❯ "` or `"  "`).
const PREFIX_W: usize = 2;

/// Terminal rows needed to show every item at `items_width`, capped at [`MAX_DROPDOWN_ROWS`].
/// Items render as flat lines (the label line plus wrapped-description continuations).
/// An item-count height therefore starves wrapped items and can leave later matches entirely off-area.
pub fn desired_item_rows(items: &[SuggestionRow], items_width: u16) -> u16 {
    if items.is_empty() {
        return 0;
    }
    flat_line_count(items, items_width as usize, MAX_DROPDOWN_ROWS as usize) as u16
}

/// The rendered `" [tag]"` suffix for a row, or `None` when untagged.
fn tag_suffix(row: &SuggestionRow) -> Option<String> {
    row.tag.as_ref().map(|t| format!(" [{t}]"))
}

/// Rendered width of a row's `" [tag]"` suffix (0 when untagged).
/// Measured without allocating: space + `[` + tag + `]` = tag width + 3.
/// The tag shares the label column so descriptions stay aligned across tagged/untagged rows.
fn tag_suffix_width(row: &SuggestionRow) -> usize {
    row.tag.as_ref().map(|t| t.width() + 3).unwrap_or(0)
}

/// Clamped name+tag per row, then the 60% budget. A short sibling cannot collapse an overlong name.
fn compute_label_column_w(items: &[SuggestionRow], content_w: usize) -> usize {
    let budget = (content_w * 3 / 5).min(LABEL_CAP);
    items
        .iter()
        .map(|r| (r.display.width() + tag_suffix_width(r)).min(LABEL_CAP))
        .max()
        .unwrap_or(0)
        .min(budget)
}

/// Build a flat list of styled lines for all visible items.
fn build_flat_lines(
    items: &[SuggestionRow],
    selected: usize,
    hovered: Option<usize>,
    label_col_w: usize,
    row_w: usize,
    theme: &Theme,
) -> (Vec<Line<'static>>, Vec<usize>) {
    let hover_bg = theme.bg_hover;
    let mut flat: Vec<Line<'static>> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        starts.push(flat.len());

        let is_selected = idx == selected;
        let is_hovered = hovered == Some(idx) && !is_selected;
        let row_bg = if is_selected {
            theme.bg_visual
        } else if is_hovered {
            hover_bg
        } else {
            theme.bg_light
        };

        let start = flat.len();
        build_item_lines(
            &mut flat,
            item,
            is_selected,
            label_col_w,
            row_w,
            row_bg,
            theme,
        );
        // Terminal theme (Reset bands): reverse video via the line style;
        // no-op on RGB themes (band bg already baked).
        {
            let overlay = if is_selected {
                Some(theme.selection_overlay())
            } else if is_hovered {
                Some(theme.hover_overlay())
            } else {
                None
            };
            if let Some(ov) = overlay {
                let Some(tail) = flat.get_mut(start..) else {
                    continue;
                };
                for line in tail {
                    line.style = line.style.patch(ov);
                }
            }
        }
    }

    (flat, starts)
}

/// This renders only the result rows (no borders or separators). Once a description wraps, rows and
/// items diverge, so callers must not use row arithmetic.
pub fn render_dropdown(
    buf: &mut Buffer,
    area: Rect,
    snap: &SlashSnapshot,
    hovered: Option<usize>,
    theme: &Theme,
) -> RenderedDropdown {
    if area.height == 0 || area.width < 4 || !snap.open {
        return RenderedDropdown::default();
    }

    let items = &snap.matches;
    let selected = snap.selected.min(items.len().saturating_sub(1));

    // Reserve 2 right columns when wrapped content overflows
    // The check runs at full width; narrowing generally adds lines
    // Dropping a badge can free width; the worst case is a spare gutter, never a missing scrollbar
    let content_w = area.width as usize;
    let visible_rows = area.height as usize;
    let needs_scrollbar = flat_line_count(items, content_w, visible_rows + 1) > visible_rows;
    let row_w = if needs_scrollbar {
        content_w.saturating_sub(2)
    } else {
        content_w
    };

    let label_col_w = compute_label_column_w(items, row_w.saturating_sub(PREFIX_W));

    // Build flat line list (multi-line descriptions produce multiple lines per item).
    let (flat_lines, item_starts) =
        build_flat_lines(items, selected, hovered, label_col_w, row_w, theme);

    // Compute scroll offset so the selected item's first line is visible.
    let selected_start = item_starts.get(selected).copied().unwrap_or(0);
    let total_lines = flat_lines.len();
    let scroll = if total_lines <= visible_rows || selected_start < visible_rows / 2 {
        0
    } else if selected_start + visible_rows / 2 >= total_lines {
        total_lines.saturating_sub(visible_rows)
    } else {
        selected_start.saturating_sub(visible_rows / 2)
    };

    // Render visible slice, recording which item each visible row shows.
    let mut row_items = Vec::with_capacity(visible_rows.min(flat_lines.len()));
    for vis_row in 0..visible_rows {
        let line_idx = scroll + vis_row;
        if line_idx >= flat_lines.len() {
            break;
        }
        row_items.push(
            item_starts
                .partition_point(|&s| s <= line_idx)
                .saturating_sub(1),
        );
        let y = area.y + vis_row as u16;
        let Some(line) = flat_lines.get(line_idx) else {
            break;
        };
        // Skip rows that fall outside the buffer (resize race).
        if y < buf.area.y || y >= buf.area.bottom() || area.x >= buf.area.right() {
            continue;
        }
        let clamped_w = row_w.min(buf.area.right().saturating_sub(area.x) as usize) as u16;
        let clamped = Rect {
            x: area.x,
            y,
            width: clamped_w,
            height: 1,
        };
        let mut row_style = line.style;
        if row_style.bg.is_none() {
            row_style = row_style.bg(theme.bg_light);
        }
        buf.set_style(clamped, row_style);
        buf.set_line_safe(area.x, y, line, row_w as u16);
    }

    if needs_scrollbar {
        // Intersect with the frame buffer so a resize race cannot paint past `buf.area` (same failure mode as item rows)
        let sb_x = area.x + area.width.saturating_sub(1);
        let sb_y = area.y.max(buf.area.y);
        let sb_bottom = (area.y.saturating_add(area.height)).min(buf.area.bottom());
        if sb_x < buf.area.right() && sb_bottom > sb_y {
            let scrollbar_area = Rect {
                x: sb_x,
                y: sb_y,
                width: 1,
                height: sb_bottom - sb_y,
            };
            let track_style = Style::default().bg(theme.bg_dark);
            let thumb_style = Style::default().fg(theme.gray_dim).bg(theme.bg_dark);
            render_scrollbar_styled(
                buf,
                Some(scrollbar_area),
                total_lines,
                scrollbar_area.height,
                scroll,
                track_style,
                thumb_style,
            );
        }
    }

    RenderedDropdown {
        row_items,
        has_scrollbar: needs_scrollbar,
    }
}

/// Hit-test geometry produced by [`render_dropdown`].
#[derive(Debug, Clone, Default)]
pub struct RenderedDropdown {
    /// Item index shown on each visible row (top to bottom).
    /// The vec is shorter than the area height when the content ends early.
    pub row_items: Vec<usize>,
    /// Whether the right 2 columns of the area are the scrollbar gutter.
    pub has_scrollbar: bool,
}

/// Badge geometry shared by [`flat_line_count`] and [`build_item_lines`] so height estimation cannot diverge from what is drawn.
struct BadgeLayout {
    badge: Option<String>,
    desc_w: usize,
}

impl BadgeLayout {
    fn compute(item: &SuggestionRow, total_w: usize, desc_indent: usize) -> Self {
        let badge = item
            .provenance
            .as_ref()
            .map(|p| p.badge().into_owned())
            .filter(|badge| desc_indent + 1 + badge.width() < total_w);
        let reserve = badge.as_ref().map_or(0, |badge| 1 + badge.width());
        let desc_w = total_w
            .saturating_sub(desc_indent)
            .saturating_sub(reserve)
            .max(1);
        Self { badge, desc_w }
    }
}

/// Flat line count of `items` at `row_w`, mirroring [`build_item_lines`] (the label line plus wrapped-description continuation lines).
/// Saturates at `cap`.
/// The empty-query dropdown lists every command; without the cap it would wrap hundreds of descriptions to compare against a single-digit height.
fn flat_line_count(items: &[SuggestionRow], row_w: usize, cap: usize) -> usize {
    let label_col_w = compute_label_column_w(items, row_w.saturating_sub(PREFIX_W));
    let desc_indent = PREFIX_W + label_col_w + LABEL_DESC_GAP;
    let mut lines = 0usize;
    for item in items {
        let desc_w = BadgeLayout::compute(item, row_w, desc_indent).desc_w;
        lines += if item.description.is_empty() {
            1
        } else {
            simple_word_wrap(&item.description, desc_w).len()
        };
        if lines >= cap {
            return cap;
        }
    }
    lines
}

/// Build lines for a single dropdown item and append them to `out`. Layout (same as question view).
/// First line: `❯ /command-name First line of description`. Continuation: ` Wrapped description
/// text`. ^indent aligned to description column.
#[allow(clippy::too_many_arguments)]
fn build_item_lines(
    out: &mut Vec<Line<'static>>,
    item: &SuggestionRow,
    is_selected: bool,
    label_col_w: usize,
    total_w: usize,
    row_bg: ratatui::style::Color,
    theme: &Theme,
) {
    let bold = if is_selected {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    let primary_fg = theme.text_primary;
    let match_fg = theme.fuzzy_accent;
    let normal_style = Style::default()
        .fg(primary_fg)
        .bg(row_bg)
        .add_modifier(bold);
    let match_style = Style::default().fg(match_fg).bg(row_bg).add_modifier(bold);
    // muted(): DIM on the terminal theme, where `gray` is Reset and the
    // description would render as heavy as the label.
    let desc_style = Style::default().fg(theme.gray).bg(row_bg);
    let bg_style = Style::default().bg(row_bg);
    let tag_style = Style::default().fg(theme.fuzzy_accent).bg(row_bg);

    // 1. Build prefix and label spans with fuzzy match highlighting.
    let prefix = if is_selected { "❯ " } else { "  " };
    let prefix_span = Span::styled(
        prefix.to_string(),
        if is_selected { normal_style } else { bg_style },
    );

    // Optional " [tag]" suffix, right-aligned at the end of the label column (just left of the description)
    // The tag is truncated and its width reserved so the name never overruns it at narrow widths
    // The suffix's leading space separates label and tag when padding is 0 (the row whose command plus tag is longest)
    let tag_text = tag_suffix(item).map(|s| truncate_str(&s, label_col_w));
    let tag_w = tag_text.as_deref().map(|s| s.width()).unwrap_or(0);

    let label = truncate_str(&item.display, label_col_w.saturating_sub(tag_w));
    let label_w = label.width();
    let padding = label_col_w.saturating_sub(label_w + tag_w);

    let label_spans = build_highlighted_spans(&label, &item.indices, normal_style, match_style);

    // `label_col_w` already includes the tag suffix, so descriptions align across tagged/untagged rows
    let desc_indent = PREFIX_W + label_col_w + LABEL_DESC_GAP;
    let layout = BadgeLayout::compute(item, total_w, desc_indent);

    let desc_lines = if item.description.is_empty() {
        Vec::new()
    } else {
        simple_word_wrap(&item.description, layout.desc_w)
    };

    // 2. First line: prefix, label, padding, [tag], gap, first desc, then the right badge.
    {
        let mut spans = vec![prefix_span];
        spans.extend(label_spans);
        if padding > 0 {
            spans.push(Span::styled(" ".repeat(padding), bg_style));
        }
        if let Some(tag_text) = tag_text {
            spans.push(Span::styled(tag_text, tag_style));
        }
        let first_desc = desc_lines.first().map(String::as_str).unwrap_or("");
        if !first_desc.is_empty() {
            spans.push(Span::styled(" ".to_string(), bg_style));
            spans.push(Span::styled(first_desc.to_string(), desc_style));
        }
        if let Some(badge) = &layout.badge {
            // Align to the first line's used width, not wrap `desc_indent`.
            let desc_gap = if first_desc.is_empty() { 0 } else { 1 };
            let used = PREFIX_W + label_col_w + desc_gap + first_desc.width();
            let gap = total_w.saturating_sub(used).saturating_sub(badge.width());
            if gap > 0 {
                spans.push(Span::styled(" ".repeat(gap), bg_style));
            }
            spans.push(Span::styled(badge.clone(), desc_style));
        }
        out.push(Line::from(spans).style(bg_style));
    }

    // 3. Continuation lines: indented to description column.
    for desc_line in desc_lines.iter().skip(1) {
        let spans = vec![
            Span::styled(" ".repeat(desc_indent), bg_style),
            Span::styled(desc_line.clone(), desc_style),
        ];
        out.push(Line::from(spans).style(bg_style));
    }
}

/// Build spans for a text string with fuzzy match character highlighting. Characters at positions
/// listed in `indices` get `match_style` (accent color), all others get `normal_style`. Adjacent
/// characters with the same style are coalesced into a single `Span` to keep the span count low.
fn build_highlighted_spans(
    text: &str,
    indices: &[u32],
    normal_style: Style,
    match_style: Style,
) -> Vec<Span<'static>> {
    if indices.is_empty() {
        return vec![Span::styled(text.to_string(), normal_style)];
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_is_match = false;
    let mut idx_iter = indices.iter().copied().peekable();

    for (char_idx, ch) in text.chars().enumerate() {
        let is_match = idx_iter.peek() == Some(&(char_idx as u32));
        if is_match {
            idx_iter.next();
        }

        if char_idx == 0 {
            current_is_match = is_match;
            current.push(ch);
        } else if is_match == current_is_match {
            current.push(ch);
        } else {
            // Style transition: flush the current run
            let style = if current_is_match {
                match_style
            } else {
                normal_style
            };
            spans.push(Span::styled(std::mem::take(&mut current), style));
            current_is_match = is_match;
            current.push(ch);
        }
    }

    if !current.is_empty() {
        let style = if current_is_match {
            match_style
        } else {
            normal_style
        };
        spans.push(Span::styled(current, style));
    }

    spans
}

/// Simple word-wrap for plain text. Returns lines of at most `width` chars.
///
/// Breaks at word boundaries when possible, hard-breaks at `width` otherwise.
fn simple_word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let normalized = text.replace('\n', " ");
    let mut remaining = normalized.as_str();
    while !remaining.is_empty() {
        if remaining.width() <= width {
            lines.push(remaining.to_string());
            break;
        }
        // Find break point: last space within width, or hard break.
        let break_at = {
            let mut last_space = None;
            let mut w = 0;
            for (i, ch) in remaining.char_indices() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if w + cw > width {
                    break;
                }
                w += cw;
                if ch == ' ' {
                    last_space = Some(i);
                }
            }
            last_space.map(|i| i + 1).unwrap_or_else(|| {
                remaining
                    .char_indices()
                    .scan(0usize, |w, (i, ch)| {
                        *w += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                        if *w > width {
                            None
                        } else {
                            Some(i + ch.len_utf8())
                        }
                    })
                    .last()
                    .unwrap_or(remaining.len())
            })
        };
        let (chunk, rest) = remaining.split_at(break_at);
        lines.push(chunk.trim_end().to_string());
        remaining = rest.trim_start();
    }
    lines
}
