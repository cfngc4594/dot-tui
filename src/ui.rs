//! Single-dot adapter over extracted Grok Build agent-view UI primitives.
//! Attribution and precise upstream modifications are recorded in upstream_ui.rs.
#[path = "upstream_shortcuts.rs"]
mod upstream_shortcuts;
#[path = "upstream_ui.rs"]
mod upstream_ui;
use super::{App, clean};
#[path = "timeline_cache.rs"]
mod timeline_cache;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use std::sync::Arc;
pub(super) use timeline_cache::TimelineCache;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use upstream_ui as upstream;

pub(super) const MIN_WIDTH: u16 = 42;
pub(super) const MIN_HEIGHT: u16 = 18;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    RetryHistory,
    Confirm,
    Cancel,
}

fn usable(area: Rect) -> bool {
    area.width >= MIN_WIDTH && area.height >= MIN_HEIGHT
}
// Adapted from xai-org/grok-build, fixed 2bdd1d6, Apache-2.0:
// crates/codegen/xai-grok-pager/src/views/shortcuts_bar.rs::ShortcutsBar::render.
// Preserve its separator and Unicode width measurement. Dot checks the WHOLE
// next hint before painting its separator, so
// a narrow footer cannot leave an orphan separator or partial hint.
const FOOTER_HINTS: [&str; 3] = ["Enter:发送", "Shift+Enter:换行", "Ctrl+Q:退出"];
const SCROLLBACK_HINTS: [&str; 2] = ["Space:输入", "Ctrl+Q:退出"];
const HINT_SEPARATOR: &str = "  │  ";
fn footer_geometry(area: Rect, hints: &[&str]) -> Vec<(Rect, Option<Rect>)> {
    let footer = upstream::compute(area, 1).footer;
    let mut x = footer.x;
    let end = footer.right();
    let mut result = Vec::new();
    for (i, text) in hints.iter().enumerate() {
        let width = text.width() as u16;
        let sep_width = if i > 0 {
            HINT_SEPARATOR.width() as u16
        } else {
            0
        };
        if x.saturating_add(sep_width).saturating_add(width) > end {
            break;
        }
        let separator = (i > 0).then(|| Rect::new(x, footer.y, sep_width, 1));
        x += sep_width;
        result.push((Rect::new(x, footer.y, width, 1), separator));
        x += width;
    }
    result
}
fn buttons(area: Rect) -> [(Action, Rect); 2] {
    [
        (
            Action::Cancel,
            Rect::new(area.x + 4, area.bottom() - 4, 10, 1),
        ),
        (
            Action::Confirm,
            Rect::new(area.right() - 16, area.bottom() - 4, 12, 1),
        ),
    ]
}
/// Geometry only. Caller MUST gate Confirm/Cancel on the currently visible
/// pending-exit modal, and all other actions on absence of a modal. Recheck the
/// actual terminal size before dispatching Confirm, and retain confirm_send's guard.
pub(super) fn hit_test(area: Rect, x: u16, y: u16) -> Option<Action> {
    if !usable(area) {
        return None;
    }
    buttons(area)
        .into_iter()
        .find_map(|(action, r)| r.contains(Position::new(x, y)).then_some(action))
}
fn button(f: &mut Frame, area: Rect, action: Action, label: &str, enabled: bool) {
    if let Some((_, r)) = buttons(area).into_iter().find(|(a, _)| *a == action) {
        f.render_widget(
            Paragraph::new(label).centered().style(if enabled {
                Style::default().fg(upstream::TEXT).bg(upstream::BASE)
            } else {
                Style::default().fg(upstream::MUTED)
            }),
            r,
        );
    }
}
// Plaintext uses the same upstream word wrapper as the Grok user prompt.
// No markdown interpretation in drafts, confirmation text, or user messages.
fn rows(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = super::grok_markdown::wrap_plain(text, width.max(1))
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|s| s.content.into_owned())
                .collect::<String>()
        })
        .collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
// EntryRenderer's short timestamp format, converted to the machine timezone.
// Invalid source timestamps remain absent; no fabricated message times.
fn timestamp(created: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created)
        .map(|value| {
            value
                .with_timezone(&chrono::Local)
                .format("%-I:%M %p")
                .to_string()
        })
        .unwrap_or_default()
}
#[cfg(test)]
fn transcript_layout(
    messages: &[super::Message],
    width: usize,
    expanded: &std::collections::HashMap<String, bool>,
) -> (Vec<Line<'static>>, Vec<timeline_cache::MessageRows>) {
    transcript_layout_media(messages, width, expanded, &Default::default(), false)
}
fn transcript_layout_media(
    messages: &[super::Message],
    width: usize,
    expanded: &std::collections::HashMap<String, bool>,
    media: &std::collections::HashMap<String, crate::upstream_media::Info>,
    inline: bool,
) -> (Vec<Line<'static>>, Vec<timeline_cache::MessageRows>) {
    let mut ranges = Vec::new();
    let mut lines = Vec::new();
    for m in messages {
        let start = lines.len();
        let is_dot = m.role == "dot";
        let bg = if is_dot {
            upstream::BASE
        } else {
            upstream::BAND
        };
        let ts = timestamp(&m.created_at);
        let reserved = upstream::timestamp_reserved(!ts.is_empty()) as usize;
        // Upstream entry chrome: three cells left, one right, plus timestamp gutter.
        let content_width = width.saturating_sub(4 + reserved).max(1);
        let mut body = clean(&m.text, true);
        let body_expanded = !m.is_human_message()
            || super::upstream_user_fold::expanded(&body, expanded.get(&m.id).copied());
        // Dot wire messages keep file IDs separately and strip local prompt
        // chips at send. Reconstruct only actual attachment chips, then use
        // the existing Grok styled user-line wrapper, not media tool blocks.
        if m.is_human_message() && !m.files.is_empty() {
            let chips = (1..=m.files.len())
                .map(|n| format!("[Image #{n}] "))
                .collect::<String>();
            body.insert_str(0, &chips);
        }
        let mut attachments = Vec::new();
        let mut claimed_images = std::collections::HashSet::new();
        let content = if is_dot {
            assistant_lines(&body, content_width)
        } else {
            super::upstream_user_fold::lines(
                &body,
                content_width.saturating_sub(2),
                if m.role == "unknown" { "? " } else { "❯ " },
                body_expanded,
            )
        };
        if !is_dot {
            lines.push(Line::styled(" ".repeat(width), Style::default().bg(bg)));
        }
        for (i, row) in content.into_iter().enumerate() {
            let row_style = row.style;
            let mut spans = vec![Span::raw("   ")];
            spans.extend(row.spans);
            let used: usize = spans.iter().map(Span::width).sum();
            let stamp = if i == 0 { ts.as_str() } else { "" };
            let stamp_width = stamp.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>();
            spans.push(Span::raw(
                " ".repeat(width.saturating_sub(used + stamp_width + 1)),
            ));
            spans.push(Span::styled(
                stamp.to_owned(),
                Style::default().fg(upstream::MUTED),
            ));
            spans.push(Span::raw(" "));
            if m.is_human_message() {
                let plain: String = spans.iter().map(|span| span.content.as_ref()).collect();
                for (index, file) in m.files.iter().enumerate() {
                    let number = index + 1;
                    if claimed_images.contains(&number) {
                        continue;
                    }
                    let token = format!("[Image #{number}]");
                    if let Some(start) = plain.find(&token) {
                        attachments.push(timeline_cache::AttachmentHit {
                            row: lines.len(),
                            column: plain[..start].width() as u16,
                            width: token.width() as u16,
                            height: 1,
                            file: file.clone(),
                            image_rows: None,
                            preview_number: Some(number),
                        });
                        claimed_images.insert(number);
                    }
                }
            }
            lines.push(
                Line::from(spans)
                    .style(Style::default().fg(upstream::TEXT).bg(bg).patch(row_style)),
            );
        }
        let mut media_ranges = Vec::new();
        for file in m.files.iter().filter(|_| !m.is_human_message()) {
            let row = lines.len();
            let available = width.saturating_sub(4).min(u16::MAX as usize) as u16;
            let (media_lines, button_row, image_rows) =
                crate::upstream_media::history_lines(file, media.get(file), available, inline);
            for mut line in media_lines {
                line.spans.insert(0, Span::raw("   "));
                let used: usize = line.spans.iter().map(Span::width).sum();
                line.spans
                    .push(Span::raw(" ".repeat(width.saturating_sub(used))));
                line.style = line.style.bg(bg);
                lines.push(line);
            }
            media_ranges.push(timeline_cache::MediaRows {
                start: row,
                button: row + button_row as usize,
            });
            let label = crate::upstream_media::media_open_button_label(false);
            if available >= label.width() as u16 {
                attachments.push(timeline_cache::AttachmentHit {
                    row: row + button_row as usize,
                    column: 3 + crate::upstream_media::media_open_button_col(available, false),
                    width: label.width() as u16,
                    height: 1,
                    image_rows: None,
                    preview_number: None,
                    file: file.clone(),
                });
            }
            if let Some(rows) = image_rows {
                attachments.push(timeline_cache::AttachmentHit {
                    row: row + 3,
                    column: 3,
                    width: available,
                    height: rows,
                    image_rows: Some(rows),
                    preview_number: None,
                    file: file.clone(),
                });
            }
        }
        if !is_dot {
            lines.push(Line::styled(" ".repeat(width), Style::default().bg(bg)));
        }
        lines.push(Line::from(""));
        ranges.push(timeline_cache::MessageRows {
            id: m.id.clone(),
            start,
            end: lines.len(),
            attachments,
            media: media_ranges,
        });
    }
    (lines, ranges)
}
#[cfg(test)]
fn transcript_messages(messages: &[super::Message], width: usize) -> Vec<Line<'static>> {
    transcript_layout(messages, width, &Default::default()).0
}
#[cfg(test)]
fn transcript(app: &App, width: usize) -> Vec<Line<'static>> {
    transcript_messages(&app.messages, width)
}
fn assistant_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    super::grok_markdown::render(text, width)
}
fn layout(app: &App, area: Rect) -> upstream::AgentLayout {
    let width = upstream::content_width(area.width.saturating_sub(4))
        .saturating_sub(2)
        .max(1);
    let text_height = app.draft.desired_height(width);
    upstream::compute(area, text_height)
}
pub(super) struct Timeline {
    pub content: Rect,
    pub track: Option<Rect>,
    pub total: usize,
    prepared: Option<Arc<timeline_cache::Prepared>>,
    pub(super) pending: bool,
}
/// Grok scrollback/state/selection.rs::capture_fold_anchor /
/// rebuild_with_fold_anchor: preserve scroll plus the entry virtual-Y delta.
/// Dot resolves against its asynchronous immutable layout snapshot.
pub(super) struct FoldAnchor {
    id: String,
    entry_y: usize,
    scroll_before: usize,
}
pub(super) fn capture_fold_anchor(app: &App, area: Rect, id: &str) -> Option<FoldAnchor> {
    let timeline = timeline(app, area);
    let entry = timeline
        .prepared
        .as_ref()?
        .messages
        .iter()
        .find(|m| m.id == id)?;
    Some(FoldAnchor {
        id: id.to_owned(),
        entry_y: entry.start,
        scroll_before: effective_scroll(app, &timeline),
    })
}
pub(super) fn resolve_fold_anchor(timeline: &Timeline, anchor: &FoldAnchor) -> Option<usize> {
    if timeline.pending {
        return None;
    }
    let entry = timeline
        .prepared
        .as_ref()?
        .messages
        .iter()
        .find(|m| m.id == anchor.id)?;
    let mut scroll = if entry.start >= anchor.entry_y {
        anchor
            .scroll_before
            .saturating_add(entry.start - anchor.entry_y)
    } else {
        anchor
            .scroll_before
            .saturating_sub(anchor.entry_y - entry.start)
    }
    .min(timeline.limit());
    // Only correct visibility if collapsing removed the entire visible fragment.
    // Human blocks end with two blank padding/separator rows.
    if entry.end.saturating_sub(2) <= scroll {
        scroll = entry.start.min(timeline.limit());
    }
    if entry.start >= scroll.saturating_add(timeline.content.height as usize) {
        scroll = entry
            .start
            .saturating_sub(timeline.content.height.saturating_sub(1) as usize)
            .min(timeline.limit());
    }
    Some(scroll)
}
impl Timeline {
    pub fn limit(&self) -> usize {
        self.total.saturating_sub(self.content.height as usize)
    }
}
// Measure first at full width, reserve the upstream gap/track only on overflow,
// then rewrap at the reduced width. All draw, hit and scroll paths use this result.
pub(super) fn timeline(app: &App, area: Rect) -> Timeline {
    app.timeline_area.set(area);
    if !usable(area) {
        return Timeline {
            content: Rect::default(),
            track: None,
            total: 0,
            prepared: None,
            pending: false,
        };
    }
    let full = layout(app, area).body;
    let (prepared, pending) = app.timeline_cache.get(
        &app.messages,
        &app.expanded_messages,
        full.width,
        full.height,
    );
    let total = prepared.as_ref().map(|p| p.lines.len()).unwrap_or(0);
    let measured = Rect {
        width: prepared.as_ref().map_or(full.width, |p| p.width),
        ..full
    };
    let (content, track) = if total > full.height as usize {
        upstream::scrollbar_geometry(area, measured)
    } else {
        (measured, None)
    };
    Timeline {
        content,
        track,
        total,
        prepared,
        pending,
    }
}
pub(super) fn user_prefix_lines(lines: Vec<Line<'static>>, prefix: &str) -> Vec<Line<'static>> {
    upstream::user_lines(lines, prefix)
}
#[cfg(test)]
pub(super) fn attachment_hit(app: &App, area: Rect, x: u16, y: u16) -> Option<String> {
    attachment_hit_with_number(app, area, x, y).map(|(file, _)| file)
}
pub(super) fn attachment_hit_with_number(
    app: &App,
    area: Rect,
    x: u16,
    y: u16,
) -> Option<(String, Option<usize>)> {
    let timeline = timeline(app, area);
    if timeline.pending || !timeline.content.contains(Position::new(x, y)) {
        return None;
    }
    let row = effective_scroll(app, &timeline) as usize + usize::from(y - timeline.content.y);
    for message in &timeline.prepared?.messages {
        for hit in &message.attachments {
            let left = timeline.content.x.saturating_add(hit.column);
            if row >= hit.row
                && row < hit.row + hit.height as usize
                && x >= left
                && x < left.saturating_add(hit.width)
            {
                return Some((hit.file.clone(), hit.preview_number));
            }
        }
    }
    None
}
pub(super) fn message_hit(app: &App, area: Rect, x: u16, y: u16) -> Option<String> {
    let timeline = timeline(app, area);
    if timeline.pending || !timeline.content.contains(Position::new(x, y)) {
        return None;
    }
    let row = effective_scroll(app, &timeline) as usize + (y - timeline.content.y) as usize;
    timeline
        .prepared?
        .messages
        .iter()
        .find(|m| row >= m.start && row < m.end)
        .map(|m| m.id.clone())
}
fn reading_anchor_from(
    prepared: &timeline_cache::Prepared,
    row: usize,
) -> Option<super::ReadingAnchor> {
    let row = row.min(prepared.lines.len().saturating_sub(1));
    let message = prepared
        .messages
        .iter()
        .find(|m| row >= m.start && row < m.end)?;
    Some(super::ReadingAnchor {
        id: message.id.clone(),
        row: row - message.start,
        media: message
            .media
            .iter()
            .enumerate()
            .rev()
            .find(|(_, m)| row >= m.start)
            .map(|(index, m)| {
                if row >= m.button {
                    (index, true, row - m.button)
                } else {
                    (index, false, row - m.start)
                }
            }),
    })
}
pub(super) fn reading_anchor_in(
    timeline: &Timeline,
    scroll: usize,
) -> Option<super::ReadingAnchor> {
    reading_anchor_from(timeline.prepared.as_ref()?, scroll as usize)
}
// Paint and hit tests resolve against their own immutable layout snapshot. A
// worker reply becoming ready between settle and draw must never flash row zero.
pub(super) fn effective_scroll(app: &App, timeline: &Timeline) -> usize {
    app.fold_anchor
        .as_ref()
        .and_then(|a| resolve_fold_anchor(timeline, a))
        .or_else(|| {
            app.reading_anchor
                .as_ref()
                .and_then(|a| resolve_anchor_from(timeline, a))
        })
        .unwrap_or(app.scroll.min(timeline.limit()))
}
pub(super) fn reading_anchor(app: &App) -> Option<super::ReadingAnchor> {
    let t = timeline(app, app.timeline_area.get());
    let offset = if t.prepared.is_some() {
        effective_scroll(app, &t)
    } else {
        app.scroll
    };
    let prepared = t
        .prepared
        .clone()
        .or_else(|| app.timeline_cache.last_prepared())?;
    reading_anchor_from(&prepared, offset as usize)
}
pub(super) fn snapshot_anchor(app: &App) -> Option<super::ReadingAnchor> {
    let prepared = app.timeline_cache.last_prepared()?;
    reading_anchor_from(&prepared, app.scroll as usize)
}
pub(super) fn resolve_anchor_from(
    timeline: &Timeline,
    anchor: &super::ReadingAnchor,
) -> Option<usize> {
    if timeline.pending {
        return None;
    }
    let prepared = timeline.prepared.as_ref()?;
    let row = prepared
        .messages
        .iter()
        .find(|m| m.id == anchor.id)
        .map(|m| {
            if let Some((index, button, offset)) = anchor.media
                && let Some(media) = m.media.get(index)
            {
                (if button { media.button } else { media.start })
                    .saturating_add(offset)
                    .min(m.end.saturating_sub(1))
            } else {
                m.start + anchor.row.min(m.end.saturating_sub(m.start + 1))
            }
        })?;
    Some(row.min(timeline.limit()))
}
pub(super) fn resolve_anchor(
    app: &App,
    area: Rect,
    anchor: &super::ReadingAnchor,
) -> Option<usize> {
    resolve_anchor_from(&timeline(app, area), anchor)
}
pub(super) fn history_rect(app: &App, area: Rect) -> Rect {
    layout(app, area).body
}
pub(super) fn status_rect(app: &App, area: Rect) -> Rect {
    layout(app, area).status
}
pub(super) fn image_preview_footer(f: &mut Frame, app: &App, area: Rect) {
    let footer = layout(app, area).footer;
    upstream_shortcuts::clear_row(f.buffer_mut(), footer);
    upstream_shortcuts::render_hint(
        f.buffer_mut(),
        footer,
        crate::i18n::label("Esc close", "Esc 关闭"),
    );
}
#[cfg(test)]
pub(super) fn settle(app: &App, area: Rect) {
    // Tests deliberately wait outside the paint path, using the real worker.
    app.timeline_cache.invalidate();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while timeline(app, area).pending {
        assert!(
            std::time::Instant::now() < deadline,
            "layout worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
fn body_area(app: &App, area: Rect) -> Rect {
    timeline(app, area).content
}
fn exit_area(area: Rect) -> Rect {
    Rect::new(
        area.x + 4,
        area.y + 4,
        area.width.saturating_sub(8),
        area.height.saturating_sub(9),
    )
}
#[cfg(test)]
fn transcript_scroll_limit(app: &App, area: Rect) -> usize {
    timeline(app, area).limit()
}
/// Height of the currently visible scroll target, using the actual layout.
pub(super) fn viewport_height(app: &App, area: Rect) -> u16 {
    if !usable(area) || app.quit_review {
        0
    } else {
        body_area(app, area).height
    }
}
/// Maximum offset for the current target; clamp before applying wheel deltas.
#[cfg(test)]
pub(super) fn scroll_limit(app: &App, area: Rect) -> usize {
    if app.quit_review {
        0
    } else {
        transcript_scroll_limit(app, area)
    }
}
pub(super) fn prompt_hit_area(app: &App, area: Rect) -> Rect {
    if usable(area) {
        layout(app, area).prompt
    } else {
        Rect::default()
    }
}
pub(super) fn cursor_position(app: &App, area: Rect) -> Option<(u16, u16)> {
    if !usable(area) || !app.can_edit() {
        return None;
    }
    let composer = layout(app, area).prompt;
    let ta = Rect::new(
        composer.x + 4,
        composer.y + 1,
        upstream::content_width(composer.width).saturating_sub(2),
        composer.height.saturating_sub(2),
    );
    app.draft
        .cursor_pos_with_state(ta, *app.draft_state.borrow())
}
pub(super) fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    app.bottom_hit.set(None);
    upstream::fill_background(f.buffer_mut(), area);
    if !usable(area) {
        if area.height > 0 {
            if let Some(status) = app.header_status(false, None) {
                f.render_widget(
                    Paragraph::new(clean(&status, false)).alignment(Alignment::Right),
                    Rect::new(area.x, area.y, area.width, 1),
                );
            }
        }
        if area.height > 1 {
            f.render_widget(
                Paragraph::new(crate::i18n::ui("请放大窗口至 42×18；Ctrl+Q 退出")),
                Rect::new(area.x, area.y + 1, area.width, area.height - 1),
            );
        }
        return;
    }
    let geometry = layout(app, area);
    let name = "dot-tui";
    f.render_widget(
        Paragraph::new(name).style(upstream_shortcuts::key_style()),
        Rect::new(
            geometry.top.x,
            geometry.top.y,
            (name.width() + 1).min((geometry.top.width / 3) as usize) as u16,
            1,
        ),
    );
    let timeline = timeline(app, area);
    app.history_layout_pending.set(timeline.pending);
    let body = timeline.content;
    let offset = effective_scroll(app, &timeline) as usize;
    let visible: Vec<Line<'static>> = timeline
        .prepared
        .as_ref()
        .map(|p| {
            p.lines
                .iter()
                .skip(offset)
                .take(body.height as usize)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    f.render_widget(Paragraph::new(visible), body);
    app.history_frame.borrow_mut().clear();
    app.visible_media.borrow_mut().clear();
    *app.history_image_hover.borrow_mut() = None;
    if let Some(prepared) = &timeline.prepared {
        for message in &prepared.messages {
            for hit in &message.attachments {
                if hit.row < offset + body.height as usize && hit.row + hit.height as usize > offset
                {
                    if let Some(number) = hit.preview_number {
                        let hit_area = Rect::new(
                            body.x + hit.column,
                            body.y + (hit.row - offset) as u16,
                            hit.width,
                            1,
                        );
                        if !timeline.pending
                            && app
                                .history_pointer
                                .get()
                                .is_some_and(|p| hit_area.contains(p))
                            && app.window_focused
                            && !app.quit_review
                            && app.pending.is_none()
                            && app.received_image.is_none()
                            && !app.image_view_loading
                            && app.picker.context.is_none()
                            && !app.language_menu.open()
                            && !app.prompt_mouse_down
                        {
                            *app.history_image_hover.borrow_mut() =
                                Some((hit.file.clone(), number));
                            app.visible_media.borrow_mut().insert(hit.file.clone());
                        }
                    } else {
                        app.visible_media.borrow_mut().insert(hit.file.clone());
                    }
                    if let Some(rows) = hit.image_rows {
                        let top = hit.row.max(offset);
                        let bottom = (hit.row + rows as usize).min(offset + body.height as usize);
                        app.history_frame
                            .borrow_mut()
                            .push(crate::inline_history::Placement {
                                // Identity follows attachment order, never wrapped screen rows.
                                instance: format!(
                                    "{}:{}",
                                    message.id,
                                    message
                                        .media
                                        .iter()
                                        .position(|m| m.start.saturating_add(3) == hit.row)
                                        .unwrap_or(0)
                                ),
                                file: hit.file.clone(),
                                area: Rect::new(
                                    body.x + hit.column,
                                    body.y + (top - offset) as u16,
                                    hit.width,
                                    (bottom - top) as u16,
                                ),
                                full_rows: rows,
                                top_crop: (top - hit.row) as u16,
                            });
                    }
                }
            }
        }
    }
    if !app.quit_review {
        super::scrollbar::render_scrollbar(
            f.buffer_mut(),
            timeline.track,
            timeline.total,
            body.height,
            offset,
            app.follow_latest,
        );
    }
    let composer = geometry.prompt;
    let input_available = app.enable_send && !app.quit_review;
    let enabled = input_available && app.prompt_focused && app.window_focused;
    f.buffer_mut().set_style(
        composer,
        Style::default()
            .fg(Color::Rgb(225, 225, 225))
            .bg(Color::Rgb(20, 20, 20)),
    );
    upstream::prompt_chrome(f.buffer_mut(), composer, enabled);
    let text_area = Rect::new(
        composer.x + 2,
        composer.y + 1,
        upstream::content_width(composer.width),
        composer.height.saturating_sub(2),
    );
    let style = Style::default().fg(if !input_available {
        Color::Rgb(108, 108, 108)
    } else {
        Color::Rgb(225, 225, 225)
    });
    f.render_widget(
        Paragraph::new("❯ ").style(Style::default().fg(if input_available {
            Color::Rgb(200, 200, 200)
        } else {
            Color::Rgb(88, 88, 88)
        })),
        Rect::new(text_area.x, text_area.y, 2, text_area.height),
    );
    let ta_area = Rect::new(
        text_area.x + 2,
        text_area.y,
        text_area.width.saturating_sub(2),
        text_area.height,
    );
    app.composer_area.set(ta_area);
    f.buffer_mut().set_style(ta_area, style);
    // Exact textarea render/caret integration used by Grok PromptWidget::draw.
    let mut state = app.draft_state.borrow_mut();
    ratatui::widgets::StatefulWidgetRef::render_ref(
        &&app.draft.textarea,
        ta_area,
        f.buffer_mut(),
        &mut state,
    );
    if !app.prompt_focused || !app.window_focused {
        upstream::recede_prompt(
            f.buffer_mut(),
            Rect::new(
                composer.x + 1,
                composer.y + 1,
                composer.width.saturating_sub(2),
                composer.height.saturating_sub(2),
            ),
        );
    }
    if enabled && let Some(pos) = app.draft.cursor_pos_with_state(ta_area, *state) {
        f.set_cursor_position(pos);
    }
    drop(state);
    let image_error = app
        .visible_media
        .borrow()
        .iter()
        .find_map(|file| app.timeline_cache.media_error(file));
    if let Some(status) = app.header_status(timeline.pending, image_error) {
        let status = crate::i18n::ui(&status);
        let status = if app.busy
            || app.image_loading > 0
            || app.image_view_loading
            || !app.history_loading.is_empty()
            || timeline.pending
        {
            let elapsed = app
                .operation
                .as_ref()
                .map_or(0, |(_, at)| at.elapsed().as_millis());
            format!("{} {status}", upstream::activity_frame(elapsed))
        } else {
            status
        };
        let left = ((name.width() + 2).min((geometry.top.width / 3) as usize)) as u16;
        let right = Rect::new(
            geometry.top.x + left,
            geometry.top.y,
            geometry.top.width.saturating_sub(left),
            1,
        );
        f.render_widget(
            Paragraph::new(clean(&status, false))
                .alignment(Alignment::Right)
                .style(Style::default().fg(upstream::MUTED)),
            right,
        );
    }
    let show_bottom = !app.follow_latest
        && offset < timeline.limit()
        && !app.quit_review
        && app.received_image.is_none()
        && !app.image_view_loading;
    if show_bottom {
        app.bottom_hit.set(crate::upstream_follow::draw(
            f.buffer_mut(),
            geometry.status,
            app.bottom_hover.get(),
        ));
    }
    footer(f, app, area, enabled);
    if app.quit_review {
        modal(f, app, area);
    }
}
fn footer_labels(app: &App) -> Vec<&'static str> {
    let mut hints = if app.prompt_focused {
        FOOTER_HINTS.to_vec()
    } else {
        SCROLLBACK_HINTS.to_vec()
    };
    if app.older_failed {
        hints.insert(0, "重试历史");
    }
    hints
        .into_iter()
        .map(|hint| match hint {
            "Enter:发送" => crate::i18n::label("Enter:send", "Enter:发送"),
            "Shift+Enter:换行" => crate::i18n::label("Shift+Enter:newline", "Shift+Enter:换行"),
            "Ctrl+Q:退出" => crate::i18n::label("Ctrl+Q:quit", "Ctrl+Q:退出"),
            "Space:输入" => crate::i18n::label("Space:prompt", "Space:输入"),
            "重试历史" => crate::i18n::label("Retry history", "重试历史"),
            _ => hint,
        })
        .collect()
}
pub(super) fn history_retry_hit(app: &App, area: Rect, x: u16, y: u16) -> bool {
    !app.quit_review
        && app.older_failed
        && usable(area)
        && footer_geometry(area, &footer_labels(app))
            .first()
            .is_some_and(|(rect, _)| rect.contains(Position::new(x, y)))
}
fn footer(f: &mut Frame, app: &App, area: Rect, _enabled: bool) {
    upstream_shortcuts::clear_row(f.buffer_mut(), upstream::compute(area, 1).footer);
    let hints = footer_labels(app);
    for (i, (rect, separator)) in footer_geometry(area, &hints).into_iter().enumerate() {
        if let Some(separator) = separator {
            f.render_widget(
                Paragraph::new(HINT_SEPARATOR).style(upstream_shortcuts::separator_style()),
                separator,
            );
        }
        upstream_shortcuts::render_hint(f.buffer_mut(), rect, hints[i]);
    }
}
fn modal(f: &mut Frame, app: &App, area: Rect) {
    let r = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
    f.render_widget(Clear, r);
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(crate::i18n::ui(" 确认退出 ")),
        r,
    );
    let text_area = exit_area(area);
    if app.quit_review {
        let text = "请求正在提交。退出不会撤回请求，结果可能不确定。\n\n草稿仅保存在当前会话。请先核对官方应用，勿自动重发。";
        f.render_widget(
            Paragraph::new(
                rows(&crate::i18n::ui(text), text_area.width as usize)
                    .into_iter()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            ),
            text_area,
        );
    }
    button(
        f,
        area,
        Action::Cancel,
        crate::i18n::label("Cancel", "取消"),
        true,
    );
    button(
        f,
        area,
        Action::Confirm,
        crate::i18n::label("Exit anyway", "仍然退出"),
        true,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_bounds_follow_visible_rows_and_modal() {
        let mut a = App::demo();
        let area = Rect::new(5, 7, 60, 18);
        a.messages[0].text = "中文内容\n\n".repeat(40);
        settle(&a, area);
        let body = body_area(&a, area);
        let expected = transcript(&a, body.width as usize)
            .len()
            .saturating_sub(body.height as usize);
        assert_eq!(scroll_limit(&a, area), expected);
        assert!(scroll_limit(&a, area).saturating_sub(3) < expected);
        assert_eq!(scroll_limit(&a, Rect::new(0, 0, 20, 5)), 0);
        a.quit_review = true;
        assert_eq!(scroll_limit(&a, area), 0);
    }
    #[test]
    fn upstream_prompt_grows_and_footer_stays_fixed() {
        let mut a = App::demo();
        let area = Rect::new(0, 0, 100, 30);
        let empty = layout(&a, area);
        assert_eq!(empty.prompt.height, 3);
        assert_eq!(empty.footer.y, area.bottom() - 2);
        a.append("first\nsecond\nthird");
        let multi = layout(&a, area);
        assert_eq!(multi.prompt.height, 5);
        assert_eq!(multi.footer, empty.footer);
        assert_eq!(multi.body.height + 2, empty.body.height);
    }
    #[test]
    fn user_band_has_padding_and_timestamp_gutter() {
        let mut a = App::demo();
        a.messages[0].role = "you".into();
        a.messages[0].text = "你好".into();
        a.messages[0].created_at = "2026-11-04T14:29:00Z".into();
        let lines = transcript(&a, 80);
        let text = lines[1]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(text.starts_with("   ❯ 你好"));
        assert!(text.ends_with(&format!("{} ", timestamp("2026-11-04T14:29:00Z"))));
        assert_eq!(lines[0].style.bg, Some(upstream::BAND));
        assert_eq!(lines[1].width(), 80);
    }
    #[test]
    fn narrow_screen_has_no_actions() {
        assert_eq!(hit_test(Rect::new(0, 0, 41, 18), 4, 14), None);
    }
    #[test]
    fn geometry_matches_buttons_and_offset() {
        let r = Rect::new(5, 7, 80, 30);
        for (action, b) in buttons(r) {
            assert_eq!(hit_test(r, b.x, b.y), Some(action));
        }
        assert_eq!(hit_test(r, 0, 0), None);
    }
    #[test]
    fn wide_text_wraps_by_cells() {
        assert_eq!(rows("中文ab", 4), vec!["中文", "ab"]);
    }
    #[test]
    fn layouts_render_without_panics() {
        for (w, h) in [(20, 5), (42, 18), (100, 30)] {
            let mut t = Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            let mut a = App::demo();
            settle(&a, Rect::new(0, 0, w, h));
            t.draw(|f| draw(f, &a)).unwrap();
            a.append("中文消息\n第二行");
            settle(&a, Rect::new(0, 0, w, h));
            t.draw(|f| draw(f, &a)).unwrap();
            a.quit_review = true;
            settle(&a, Rect::new(0, 0, w, h));
            t.draw(|f| draw(f, &a)).unwrap();
        }
    }
}

#[cfg(test)]
#[path = "ui_regression_tests.rs"]
mod ui_regression_tests;

#[cfg(test)]
mod inline_media_tests {
    use super::*;
    use crate::upstream_media::Info;
    fn message(role: &str) -> super::super::Message {
        super::super::Message {
            id: "m".into(),
            role: role.into(),
            text: "fixture".into(),
            created_at: String::new(),
            files: vec!["f".into()],
        }
    }
    #[test]
    fn original_image_geometry_reserves_real_rows_only_when_ready_and_supported() {
        let info = Info {
            error: None,
            name: "checkerboard.png".into(),
            dimensions: (240, 160),
        };
        let media = std::collections::HashMap::from([("f".into(), info.clone())]);
        for width in [18, 32, 80, 120] {
            let (_, fallback) = transcript_layout_media(
                &[message("dot")],
                width,
                &Default::default(),
                &media,
                false,
            );
            assert!(
                fallback[0]
                    .attachments
                    .iter()
                    .all(|a| a.image_rows.is_none())
            );
            let (_, ready) = transcript_layout_media(
                &[message("dot")],
                width,
                &Default::default(),
                &media,
                true,
            );
            let image = ready[0]
                .attachments
                .iter()
                .find(|a| a.image_rows.is_some())
                .unwrap();
            assert_eq!(
                image.height,
                crate::upstream_media::reserved_rows(&info, width.saturating_sub(4) as u16)
            );
            let button = ready[0]
                .attachments
                .iter()
                .find(|a| a.image_rows.is_none())
                .unwrap();
            assert_eq!(button.row, image.row + image.height as usize + 1);
            let (_, loading) = transcript_layout_media(
                &[message("dot")],
                width,
                &Default::default(),
                &Default::default(),
                true,
            );
            assert!(
                loading[0]
                    .attachments
                    .iter()
                    .all(|a| a.image_rows.is_none())
            );
        }
    }
    #[test]
    fn all_media_cells_and_empty_rows_have_the_message_background() {
        use ratatui::{
            buffer::Buffer,
            widgets::{Paragraph, Widget},
        };
        let media = std::collections::HashMap::from([(
            "f".into(),
            Info {
                error: None,
                name: "alpha.png".into(),
                dimensions: (240, 160),
            },
        )]);
        // Human attachments are chips now; inline media belongs to dot messages.
        for role in ["dot"] {
            for inline in [true, false] {
                let (lines, ranges) = transcript_layout_media(
                    &[message(role)],
                    80,
                    &Default::default(),
                    &media,
                    inline,
                );
                let first = ranges[0]
                    .attachments
                    .iter()
                    .map(|a| a.row)
                    .min()
                    .unwrap()
                    .saturating_sub(3);
                let end = ranges[0].end - 2;
                let mut b = Buffer::empty(Rect::new(0, 0, 80, lines.len() as u16));
                Paragraph::new(lines).render(b.area, &mut b);
                for y in first..end {
                    for x in 0..80 {
                        assert_eq!(
                            b[(x, y as u16)].bg,
                            if role == "you" {
                                upstream::BAND
                            } else {
                                upstream::BASE
                            },
                            "{x},{y}"
                        );
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod media_anchor_tests {
    use super::*;
    #[test]
    fn asynchronous_growth_before_second_attachment_preserves_its_button_anchor() {
        let mut m = crate::backend::Message {
            id: "m".into(),
            role: "dot".into(),
            text: "fixture".into(),
            created_at: String::new(),
            files: vec![],
        };
        m.files = vec!["first".into(), "second".into()];
        let (lines, messages) = transcript_layout_media(
            std::slice::from_ref(&m),
            80,
            &Default::default(),
            &Default::default(),
            true,
        );
        let row = messages[0].media[1].button;
        let old = timeline_cache::Prepared {
            width: 80,
            lines,
            messages,
        };
        let anchor = reading_anchor_from(&old, row).unwrap();
        let media = std::collections::HashMap::from([
            (
                "first".into(),
                crate::upstream_media::Info {
                    error: None,
                    name: "wide.png".into(),
                    dimensions: (480, 96),
                },
            ),
            (
                "second".into(),
                crate::upstream_media::Info {
                    error: None,
                    name: "portrait.png".into(),
                    dimensions: (96, 400),
                },
            ),
        ]);
        let (lines, messages) =
            transcript_layout_media(&[m], 80, &Default::default(), &media, true);
        let expected = messages[0].media[1].button;
        let timeline = Timeline {
            content: Rect::new(0, 0, 80, 2),
            track: None,
            total: lines.len(),
            prepared: Some(Arc::new(timeline_cache::Prepared {
                width: 80,
                lines,
                messages,
            })),
            pending: false,
        };
        assert_eq!(resolve_anchor_from(&timeline, &anchor), Some(expected));
    }
}
#[cfg(test)]
mod media_hit_tests {
    use super::*;
    #[test]
    fn cropped_pixels_and_open_button_use_same_media_hit_and_viewport() {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        let mut app = App::demo();
        app.messages = vec![crate::backend::Message {
            id: "m".into(),
            role: "dot".into(),
            text: "fixture".into(),
            created_at: String::new(),
            files: vec!["f".into()],
        }];
        app.timeline_cache.set_media(
            "f".into(),
            crate::upstream_media::Info {
                error: None,
                name: "portrait.png".into(),
                dimensions: (96, 400),
            },
        );
        let area = Rect::new(0, 0, 100, 24);
        settle(&app, area);
        let t = timeline(&app, area);
        let p = t.prepared.as_ref().unwrap();
        let image = p.messages[0]
            .attachments
            .iter()
            .find(|a| a.image_rows.is_some())
            .unwrap();
        app.scroll = image.row + 3;
        app.follow_latest = false;
        app.prompt_focused = false;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| crate::draw(f, &app)).unwrap();
        let placements = app.history_frame.borrow();
        assert_eq!(placements.len(), 1);
        let placement = &placements[0];
        assert_eq!(placement.top_crop, 3);
        assert_eq!(placement.area.y, t.content.y);
        assert!(placement.area.bottom() <= t.content.bottom());
        assert_eq!(
            attachment_hit(&app, area, placement.area.x, placement.area.y),
            Some("f".into())
        );
        assert_eq!(
            attachment_hit(&app, area, placement.area.x, t.content.bottom()),
            None
        );
        drop(placements);
        app.scroll = t.limit();
        terminal.draw(|f| crate::draw(f, &app)).unwrap();
        let t = timeline(&app, area);
        let button = t.prepared.as_ref().unwrap().messages[0]
            .attachments
            .iter()
            .find(|a| a.image_rows.is_none())
            .unwrap();
        let y = t.content.y + (button.row - app.scroll as usize) as u16;
        assert_eq!(
            attachment_hit(&app, area, t.content.x + button.column, y),
            Some("f".into())
        );
    }
}
