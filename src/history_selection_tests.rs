//! Real mouse dispatch with synthetic history and the default in-memory clipboard.
use super::*;
use crate::scrollback::text_selection::ResolvedSelectableLine;
use xai_ratatui_textarea::ClipboardProvider;

fn fixture(text: &str, area: Rect) -> App {
    let mut app = App::demo();
    app.messages = vec![Message {
        id: "selection-fixture".into(),
        role: "dot".into(),
        text: text.into(),
        files: vec![],
        created_at: "2026-11-04T06:29:00Z".into(),
    }];
    app.append("unsent 中文 draft");
    app.clipboard.set("clipboard sentinel");
    ui::settle(&app, area);
    app
}
fn lines(app: &App, area: Rect) -> Vec<ResolvedSelectableLine> {
    ui::selection_model(app, area)
        .0
        .ranges
        .into_iter()
        .flat_map(|range| range.lines)
        .collect()
}
fn human_fixture(text: &str, area: Rect) -> App {
    let mut app = fixture(text, area);
    app.messages[0].role = "you".into();
    app.expanded_messages
        .insert("selection-fixture".into(), true);
    app.history_changed();
    ui::settle(&app, area);
    app
}
fn start(line: &ResolvedSelectableLine) -> (u16, u16) {
    (line.screen_x + line.selectable_cols.start, line.screen_y)
}
fn end(line: &ResolvedSelectableLine) -> (u16, u16) {
    (line.screen_x + line.selectable_cols.end, line.screen_y)
}
fn pointer(
    app: &mut App,
    area: Rect,
    kind: MouseEventKind,
    at: (u16, u16),
    tx: &mpsc::Sender<Job>,
) {
    dispatch_input(
        app,
        Event::Mouse(MouseEvent {
            kind,
            column: at.0,
            row: at.1,
            modifiers: KeyModifiers::NONE,
        }),
        area,
        area,
        tx,
    );
}
fn drag(app: &mut App, area: Rect, from: (u16, u16), to: (u16, u16), tx: &mpsc::Sender<Job>) {
    pointer(app, area, MouseEventKind::Down(MouseButton::Left), from, tx);
    pointer(app, area, MouseEventKind::Drag(MouseButton::Left), to, tx);
}

#[test]
fn chinese_wide_combining_drag_copies_both_directions_without_editing_or_sending() {
    let area = Rect::new(0, 0, 100, 30);
    let text = "中文宽字符 👩‍💻 e\u{301} 原样复制!";
    for reverse in [false, true] {
        let (tx, rx) = mpsc::channel();
        let mut app = fixture(text, area);
        let row = lines(&app, area)
            .into_iter()
            .find(|l| l.text.contains("中文"))
            .unwrap();
        // The last text boundary is outside the hit-test's half-open span.
        let (from, to) = if reverse {
            ((end(&row).0 - 1, row.screen_y), start(&row))
        } else {
            (start(&row), end(&row))
        };
        drag(&mut app, area, from, to, &tx);
        pointer(
            &mut app,
            area,
            MouseEventKind::Up(MouseButton::Left),
            to,
            &tx,
        );
        assert_eq!(app.clipboard.get().as_deref(), Some(text));
        assert_eq!(app.draft(), "unsent 中文 draft");
        assert!(rx.try_recv().is_err());
        assert!(app.history_press.is_none());
    }
}

#[test]
fn soft_wraps_and_hard_breaks_copy_body_without_chrome() {
    let area = Rect::new(0, 0, 60, 40);
    let text = format!("{}\nsecond hard line\n第三行", "abcdefghij".repeat(10));
    let (tx, rx) = mpsc::channel();
    let mut app = human_fixture(&text, area);
    let rows = lines(&app, area);
    assert!(rows.len() > 3, "fixture must exercise soft wrapping");
    let from = start(rows.first().unwrap());
    let to = end(rows.last().unwrap());
    drag(&mut app, area, from, to, &tx);
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        to,
        &tx,
    );
    assert_eq!(app.clipboard.get().as_deref(), Some(text.as_str()));
    assert!(rx.try_recv().is_err());
}

#[test]
fn bare_moved_recovers_missing_mouse_up_and_copies_once() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, _) = mpsc::channel();
    let mut app = fixture("lost release recovery", area);
    let row = lines(&app, area).remove(0);
    drag(&mut app, area, start(&row), end(&row), &tx);
    pointer(&mut app, area, MouseEventKind::Moved, end(&row), &tx);
    assert_eq!(
        app.clipboard.get().as_deref(),
        Some("lost release recovery")
    );
    assert!(!app.history_selection.borrow().latched());
    app.clipboard.set("after copy");
    pointer(&mut app, area, MouseEventKind::Moved, start(&row), &tx);
    assert_eq!(app.clipboard.get().as_deref(), Some("after copy"));
}

#[test]
fn focus_loss_and_resize_cancel_drag_without_copying() {
    let area = Rect::new(0, 0, 100, 30);
    for interruption in [Event::FocusLost, Event::Resize(90, 28)] {
        let (tx, _) = mpsc::channel();
        let mut app = fixture("cancel selection safely", area);
        let row = lines(&app, area).remove(0);
        drag(&mut app, area, start(&row), end(&row), &tx);
        assert!(app.history_selection.borrow().active.is_some());
        dispatch_input(&mut app, interruption, area, area, &tx);
        pointer(
            &mut app,
            area,
            MouseEventKind::Up(MouseButton::Left),
            end(&row),
            &tx,
        );
        assert!(!app.history_selection.borrow().latched());
        assert_eq!(app.clipboard.get().as_deref(), Some("clipboard sentinel"));
        assert_eq!(app.draft(), "unsent 中文 draft");
    }
}

#[test]
fn replacement_async_snapshot_cannot_copy_another_message() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, _) = mpsc::channel();
    let mut app = fixture("original selected message", area);
    let row = lines(&app, area).remove(0);
    drag(&mut app, area, start(&row), end(&row), &tx);
    app.messages[0].id = "replacement".into();
    app.messages[0].text = "different message must never be copied".into();
    // Exercise snapshot protection independently of eager cancellation.
    app.timeline_cache.invalidate();
    ui::settle(&app, area);
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        end(&row),
        &tx,
    );
    assert_eq!(app.clipboard.get().as_deref(), Some("clipboard sentinel"));
    assert!(!app.history_selection.borrow().latched());
}

#[test]
fn edge_drag_autoscroll_is_bounded_and_stops_after_release() {
    let area = Rect::new(0, 0, 80, 25);
    let (tx, _) = mpsc::channel();
    let text = (0..80)
        .map(|i| format!("line {i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = human_fixture(&text, area);
    app.follow_latest = false;
    app.scroll = 10;
    let rows = lines(&app, area);
    let body = ui::selection_model(&app, area).0.content_area;
    let from = start(&rows[2]);
    let to = (from.0, body.bottom().saturating_add(2));
    drag(&mut app, area, from, to, &tx);
    let before = app.scroll;
    let now = Instant::now();
    history_selection::tick(&mut app, area, now);
    assert!(app.scroll > before);
    assert!(app.scroll <= ui::scroll_limit(&app, area));
    let once = app.scroll;
    history_selection::tick(&mut app, area, now);
    assert_eq!(app.scroll, once, "timer must throttle repeated ticks");
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        to,
        &tx,
    );
    history_selection::tick(&mut app, area, now + Duration::from_secs(1));
    assert_eq!(app.scroll, once);
    assert_ne!(app.clipboard.get().as_deref(), Some("clipboard sentinel"));
}

#[test]
fn gutter_click_does_not_copy_and_double_click_still_folds_human_message() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, rx) = mpsc::channel();
    let mut app = fixture(&"foldable human message\n".repeat(30), area);
    app.messages[0].role = "you".into();
    app.history_changed();
    ui::settle(&app, area);
    // WordSelect owns body double-clicks. Upstream leaves fold clicks on the
    // non-text prompt prefix/gutter, so exercise that unchanged fallback here.
    let first = lines(&app, area).remove(0);
    let at = (first.screen_x - 1, first.screen_y);
    for _ in 0..2 {
        pointer(
            &mut app,
            area,
            MouseEventKind::Down(MouseButton::Left),
            at,
            &tx,
        );
        pointer(
            &mut app,
            area,
            MouseEventKind::Up(MouseButton::Left),
            at,
            &tx,
        );
    }
    assert!(app.expanded_messages.contains_key("selection-fixture"));
    assert_eq!(app.clipboard.get().as_deref(), Some("clipboard sentinel"));
    assert!(rx.try_recv().is_err());
}

#[test]
fn image_click_keeps_attachment_routing_without_starting_text_drag() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, rx) = mpsc::channel();
    let mut app = fixture("message with an image", area);
    app.messages[0].files = vec!["fake-file-1".into()];
    app.history_changed();
    ui::settle(&app, area);
    let at = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| ui::attachment_hit(&app, area, x, y).is_some())
        .expect("attachment has a clickable region");
    pointer(
        &mut app,
        area,
        MouseEventKind::Down(MouseButton::Left),
        at,
        &tx,
    );
    assert!(app.image_view_loading);
    assert!(!app.history_selection.borrow().latched());
    assert_eq!(app.clipboard.get().as_deref(), Some("clipboard sentinel"));
    assert_eq!(app.draft(), "unsent 中文 draft");
    let job = rx.try_recv().expect("image click requests loading");
    assert!(!matches!(job, Job::Send(..)));
    assert!(rx.try_recv().is_err());
}

#[test]
fn prompt_click_returns_ctrl_c_ownership_and_never_recopies_history() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, rx) = mpsc::channel();
    let mut app = fixture("previous history selection", area);
    let row = lines(&app, area).remove(0);
    drag(&mut app, area, start(&row), end(&row), &tx);
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        end(&row),
        &tx,
    );
    assert_eq!(
        app.clipboard.get().as_deref(),
        Some("previous history selection")
    );
    app.clipboard.set("new clipboard owner");
    let prompt = ui::prompt_hit_area(&app, area);
    let at = (prompt.x + 1, prompt.y);
    pointer(
        &mut app,
        area,
        MouseEventKind::Down(MouseButton::Left),
        at,
        &tx,
    );
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        at,
        &tx,
    );
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &tx,
    );
    assert_eq!(app.clipboard.get().as_deref(), Some("new clipboard owner"));
    assert!(
        app.draft().is_empty(),
        "prompt Ctrl+C must keep its original clear-draft behavior"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn blank_padding_drag_defers_anchor_until_it_enters_text() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, rx) = mpsc::channel();
    let mut app = human_fixture("deferred text anchor", area);
    let row = lines(&app, area).remove(0);
    let model = ui::selection_model(&app, area).0;
    let block = &model.visible_blocks[0];
    let margin = (
        start(&row).0,
        (block.area.y..block.area.bottom())
            .find(|&y| model.hit_test_selectable_range(start(&row).0, y).is_none())
            .expect("human band has non-selectable padding"),
    );
    drag(&mut app, area, margin, start(&row), &tx);
    pointer(
        &mut app,
        area,
        MouseEventKind::Drag(MouseButton::Left),
        end(&row),
        &tx,
    );
    pointer(
        &mut app,
        area,
        MouseEventKind::Up(MouseButton::Left),
        end(&row),
        &tx,
    );
    assert_eq!(app.clipboard.get().as_deref(), Some("deferred text anchor"));
    assert_eq!(app.draft(), "unsent 中文 draft");
    assert!(rx.try_recv().is_err());
}
