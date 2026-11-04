//! Single-dot adapter over extracted Grok Build agent-view UI primitives.
//! Attribution and precise upstream modifications are recorded in upstream_ui.rs.
#[path = "upstream_ui.rs"]
mod upstream_ui;
use super::{clean, App};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthChar;
use upstream_ui as upstream;

pub(super) const MIN_WIDTH: u16 = 42;
pub(super) const MIN_HEIGHT: u16 = 18;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Refresh,
    Earlier,
    Review,
    Confirm,
    Cancel,
}

fn usable(area: Rect) -> bool {
    area.width >= MIN_WIDTH && area.height >= MIN_HEIGHT
}
fn buttons(area: Rect) -> [(Action, Rect); 5] {
    [
        (
            Action::Earlier,
            Rect::new(area.x + 2, area.bottom() - 2, 8, 1),
        ),
        (
            Action::Refresh,
            Rect::new(area.x + 12, area.bottom() - 2, 8, 1),
        ),
        (
            Action::Review,
            Rect::new(area.right() - 12, area.bottom() - 2, 10, 1),
        ),
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
/// review/quit modal, and all other actions on absence of a modal. Recheck the
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
fn transcript(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for m in &app.messages {
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
        let body = clean(&m.text, true);
        let content = if is_dot {
            assistant_lines(&body, content_width)
        } else {
            let wrapped = rows(&body, content_width.saturating_sub(2))
                .into_iter()
                .map(|s| Line::styled(s, Style::default().fg(upstream::TEXT)))
                .collect();
            upstream::user_lines(wrapped, if m.role == "unknown" { "? " } else { "❯ " })
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
            lines.push(
                Line::from(spans)
                    .style(Style::default().fg(upstream::TEXT).bg(bg).patch(row_style)),
            );
        }
        if !is_dot {
            lines.push(Line::styled(" ".repeat(width), Style::default().bg(bg)));
        }
        lines.push(Line::from(""));
    }
    lines
}
fn assistant_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    super::grok_markdown::render(text, width)
}
fn layout(app: &App, area: Rect) -> upstream::AgentLayout {
    let width = upstream::content_width(area.width.saturating_sub(4))
        .saturating_sub(2)
        .max(1);
    let text_height =
        super::editor::layout(app.draft(), app.draft_cursor(), width as usize, usize::MAX)
            .rows
            .len()
            .min(u16::MAX as usize) as u16;
    upstream::compute(area, text_height)
}
pub(super) struct Timeline {
    pub content: Rect,
    pub track: Option<Rect>,
    pub total: u16,
}
impl Timeline {
    pub fn limit(&self) -> u16 {
        self.total.saturating_sub(self.content.height)
    }
}
// Measure first at full width, reserve the upstream gap/track only on overflow,
// then rewrap at the reduced width. All draw, hit and scroll paths use this result.
pub(super) fn timeline(app: &App, area: Rect) -> Timeline {
    if !usable(area) {
        return Timeline {
            content: Rect::default(),
            track: None,
            total: 0,
        };
    }
    let full = layout(app, area).body;
    let total = transcript(app, full.width as usize)
        .len()
        .min(u16::MAX as usize) as u16;
    let (content, track) = super::scrollbar::maybe_split_for_scrollbar(full, total);
    let total = if track.is_some() {
        transcript(app, content.width as usize)
            .len()
            .min(u16::MAX as usize) as u16
    } else {
        total
    };
    Timeline {
        content,
        track,
        total,
    }
}
fn body_area(app: &App, area: Rect) -> Rect {
    timeline(app, area).content
}
pub(super) fn input_width(app: &App, area: Rect) -> usize {
    upstream::content_width(layout(app, area).prompt.width)
        .saturating_sub(2)
        .max(1) as usize
}
fn review_area(area: Rect) -> Rect {
    Rect::new(
        area.x + 4,
        area.y + 4,
        area.width.saturating_sub(8),
        area.height.saturating_sub(9),
    )
}
fn review_rows(app: &App, width: usize) -> Vec<String> {
    let Some(plan) = &app.review else {
        return Vec::new();
    };
    let name: String = clean(&plan.dot.name, false).chars().take(80).collect();
    let id: String = clean(&plan.dot.id, false).chars().take(36).collect();
    let mode = if app.live {
        "真实发送，可能触发任务；发送后无法修改。"
    } else {
        "离线模拟，不访问真实账户。"
    };
    rows(
        &format!("目标：{name} [{id}]\n{mode}\n\n{}", clean(&plan.text, true)),
        width,
    )
}
fn transcript_scroll_limit(app: &App, area: Rect) -> u16 {
    timeline(app, area).limit()
}
/// Height of the currently visible scroll target, using the actual layout.
pub(super) fn viewport_height(app: &App, area: Rect) -> u16 {
    if !usable(area) || app.quit_review {
        0
    } else if app.review.is_some() {
        review_area(area).height
    } else {
        body_area(app, area).height
    }
}
/// Maximum offset for the current target; clamp before applying wheel deltas.
pub(super) fn scroll_limit(app: &App, area: Rect) -> u16 {
    if app.quit_review {
        0
    } else if app.review.is_some() {
        review_scroll_limit(app, area)
    } else {
        transcript_scroll_limit(app, area)
    }
}
pub(super) fn review_scroll_limit(app: &App, area: Rect) -> u16 {
    if !usable(area) || app.review.is_none() {
        return 0;
    }
    let text = review_area(area);
    review_rows(app, text.width as usize)
        .len()
        .saturating_sub(text.height as usize)
        .min(u16::MAX as usize) as u16
}
pub(super) fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    upstream::fill_background(f.buffer_mut(), area);
    if !usable(area) {
        f.render_widget(
            Paragraph::new(format!(
                "{}\n{}\n请放大窗口至 42×18；禁止发送确认。Ctrl+Q 退出",
                clean(&app.status_line(), false),
                clean(&app.last_result, false)
            )),
            area,
        );
        return;
    }
    let geometry = layout(app, area);
    let name = app
        .dots
        .get(app.selected)
        .map(|d| clean(&d.name, false))
        .unwrap_or_else(|| "dot".into());
    let mode = if app.worker_failed {
        "连接中断"
    } else if !app.live {
        "离线演示"
    } else if app.uncertain {
        "发送已锁定"
    } else if app.pending.is_some() {
        "正在发送…"
    } else if app.busy {
        "正在读取…"
    } else if app.poll_paused {
        "读取失败 / 已暂停"
    } else if app.enable_send {
        "已连接"
    } else {
        "只读"
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(name, Style::default().fg(upstream::TEXT)),
            Span::styled(format!("  ·  {mode}"), Style::default().fg(upstream::MUTED)),
        ])),
        geometry.top,
    );
    let timeline = timeline(app, area);
    let body = timeline.content;
    let lines = transcript(app, body.width as usize);
    let max_scroll = transcript_scroll_limit(app, area);
    f.render_widget(
        Paragraph::new(lines).scroll((app.scroll.min(max_scroll), 0)),
        body,
    );
    if app.review.is_none() && !app.quit_review {
        super::scrollbar::render_scrollbar(
            f.buffer_mut(),
            timeline.track,
            timeline.total,
            body.height,
            app.scroll.min(max_scroll),
            app.follow_latest,
        );
    }
    let composer = geometry.prompt;
    let enabled = app.can_edit();
    upstream::prompt_chrome(f.buffer_mut(), composer);
    let text_area = Rect::new(
        composer.x + 3,
        composer.y + 1,
        upstream::content_width(composer.width),
        composer.height.saturating_sub(2),
    );
    let draft = clean(app.draft(), true);
    let placeholder = if app.uncertain {
        "发送已锁定，请核对官方应用"
    } else if !app.enable_send {
        "只读模式"
    } else {
        "给 dot 发消息"
    };
    let editor = super::editor::layout(
        &draft,
        app.draft_cursor(),
        text_area.width.saturating_sub(2) as usize,
        text_area.height as usize,
    );
    let display = if draft.is_empty() {
        vec![placeholder.to_owned()]
    } else {
        editor.rows
    };
    let offset = editor.offset.min(u16::MAX as usize) as u16;
    let style = Style::default().fg(if draft.is_empty() || !enabled {
        upstream::MUTED
    } else {
        upstream::TEXT
    });
    let content = upstream::user_lines(
        display
            .into_iter()
            .map(|s| Line::styled(s, style))
            .collect(),
        "❯ ",
    );
    f.render_widget(Paragraph::new(content).scroll((offset, 0)), text_area);
    if enabled && app.review.is_none() && !app.quit_review {
        f.set_cursor_position((
            text_area.x + 2 + editor.cursor.1 as u16,
            text_area.y + (editor.cursor.0 - editor.offset) as u16,
        ));
    }
    // Upstream caption sits on the bottom chrome, not in an extra status row.
    let caption = clean(&app.last_result, false);
    let caption = rows(&caption, composer.width.saturating_sub(8) as usize)
        .into_iter()
        .next()
        .unwrap_or_default();
    let cw = caption
        .chars()
        .map(|c| c.width().unwrap_or(0))
        .sum::<usize>() as u16;
    f.render_widget(
        Paragraph::new(format!(" {caption} "))
            .style(Style::default().fg(upstream::MUTED).bg(upstream::BASE)),
        Rect::new(
            composer.right().saturating_sub(cw + 4),
            composer.bottom() - 1,
            cw + 2,
            1,
        ),
    );
    let status = if app.busy || app.worker_failed || app.poll_paused || app.uncertain {
        app.status_line()
    } else if !app.live {
        "开发预览".to_owned()
    } else if app.status.starts_with("已读取") {
        "就绪".to_owned()
    } else {
        app.status_line()
    };
    f.render_widget(
        Paragraph::new(format!(
            "{} {}",
            app.operation
                .as_ref()
                .map(|(_, start)| upstream::activity_frame(start.elapsed().as_millis()))
                .unwrap_or("·"),
            clean(&status, false)
        ))
        .style(Style::default().fg(if app.uncertain {
            Color::Yellow
        } else {
            upstream::MUTED
        })),
        geometry.status,
    );
    button(
        f,
        area,
        Action::Earlier,
        "更早",
        !app.busy && !app.worker_failed && !app.older_exhausted,
    );
    button(
        f,
        area,
        Action::Refresh,
        "刷新",
        !app.busy && !app.worker_failed,
    );
    button(
        f,
        area,
        Action::Review,
        "审阅发送",
        enabled && !app.busy && !app.draft().trim().is_empty(),
    );
    // Grok-style quiet shortcut separators, never filled button boxes.
    for x in [area.x + 11, area.x + 22] {
        f.render_widget(
            Paragraph::new("│").style(Style::default().fg(upstream::BORDER)),
            Rect::new(x, geometry.footer.y, 1, 1),
        );
    }
    if area.width >= 64 {
        let hint = if area.width >= 88 {
            "Enter:换行   Ctrl+Q:退出"
        } else {
            "Enter:换行"
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().fg(upstream::MUTED)),
            Rect::new(
                area.x + 24,
                geometry.footer.y,
                area.width.saturating_sub(38),
                1,
            ),
        );
    }
    if app.review.is_some() || app.quit_review {
        modal(f, app, area);
    }
}
fn modal(f: &mut Frame, app: &App, area: Rect) {
    let r = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
    f.render_widget(Clear, r);
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(if app.quit_review {
                " 确认退出 "
            } else {
                " 审阅后发送 "
            }),
        r,
    );
    let text_area = review_area(area);
    if app.quit_review {
        let text = "请求正在提交。退出不会撤回请求，结果可能不确定。\n\n重启后将锁定发送，请先核对官方应用。";
        f.render_widget(
            Paragraph::new(
                rows(text, text_area.width as usize)
                    .into_iter()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            ),
            text_area,
        );
    } else if app.review.is_some() {
        let lines = review_rows(app, text_area.width as usize);
        let max = review_scroll_limit(app, area);
        f.render_widget(
            Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
                .scroll((app.review_scroll.min(max), 0)),
            text_area,
        );
    }
    button(f, area, Action::Cancel, "取消", true);
    button(
        f,
        area,
        Action::Confirm,
        if app.quit_review {
            "仍然退出"
        } else {
            "确认发送"
        },
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
        let body = body_area(&a, area);
        let expected = transcript(&a, body.width as usize)
            .len()
            .saturating_sub(body.height as usize) as u16;
        assert_eq!(scroll_limit(&a, area), expected);
        assert!(scroll_limit(&a, area).saturating_sub(3) < expected);
        a.append(&"审阅正文\n".repeat(30));
        a.review_send();
        let expected_review = review_rows(&a, 52).len().saturating_sub(9) as u16;
        assert_eq!(review_scroll_limit(&a, area), expected_review);
        assert_eq!(scroll_limit(&a, area), expected_review);
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
            t.draw(|f| draw(f, &a)).unwrap();
            a.append("中文消息\n第二行");
            a.review_send();
            t.draw(|f| draw(f, &a)).unwrap();
            a.quit_review = true;
            t.draw(|f| draw(f, &a)).unwrap();
        }
    }
}

#[cfg(test)]
#[path = "ui_regression_tests.rs"]
mod ui_regression_tests;
