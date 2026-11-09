use super::*;
#[test]
fn connecting_draft_paints_immediately_survives_replies_and_has_no_unknown_recipient_send() {
    let mut app = App::new();
    app.begin("offline connection wait");
    let (tx, rx) = mpsc::channel();
    app.append("连接前中文 e\u{301}");
    let cursor = app.draft_cursor();
    app.submit(&tx);
    assert!(rx.try_recv().is_err());
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| draw(f, &app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains("连接前中文")
    );
    app.apply(Reply::Dots(Ok(vec![demo_dot()])), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Page(..))));
    app.apply(Reply::Page(0, false, Ok(demo_messages())), &tx);
    assert_eq!(app.draft(), "连接前中文 e\u{301}");
    assert_eq!(app.draft_cursor(), cursor);
    assert!(rx.try_recv().is_err());
}
#[test]
fn ambiguous_or_failed_connection_keeps_draft_but_cannot_send() {
    let mut app = App::new();
    let (tx, rx) = mpsc::channel();
    app.append("keep safely");
    app.apply(Reply::Dots(Ok(vec![demo_dot(), demo_dot()])), &tx);
    app.submit(&tx);
    assert!(rx.try_recv().is_err());
    app.worker_failure("offline synthetic failure");
    app.append(" edited");
    app.submit(&tx);
    assert_eq!(app.draft(), "keep safely edited");
    assert!(rx.try_recv().is_err());
}

#[test]
fn steady_history_does_not_rebuild_for_input_or_idle_paints() {
    let mut app = App::demo();
    app.messages[0].text = "**rich** `code`\n".repeat(100);
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&app, area);
    let submitted = app.timeline_cache.submitted();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    for _ in 0..30 {
        app.append("中");
        terminal.draw(|f| draw(f, &app)).unwrap();
    }
    assert_eq!(app.timeline_cache.submitted(), submitted);
}

#[test]
fn original_coalescer_preserves_dot_command_boundaries() {
    use upstream_input::{TimedInputEvent as T, normalize_batch};
    let ctrl_s = Event::Key(event::KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    ));
    let y = Event::Key(event::KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    let normalized = normalize_batch(vec![
        T::now(Event::Paste("literal".into())),
        T::now(ctrl_s.clone()),
        T::now(y.clone()),
    ]);
    assert_eq!(normalized.len(), 3);
    assert_eq!(normalized[1].event, ctrl_s);
    assert_eq!(normalized[2].event, y);
    let normalized = normalize_batch(vec![
        T::now(Event::Key(event::KeyEvent::from(KeyCode::Char('a')))),
        T::now(Event::Key(event::KeyEvent::from(KeyCode::Enter))),
        T::now(Event::Key(event::KeyEvent::from(KeyCode::Char('b')))),
    ]);
    assert_eq!(normalized.len(), 3);
    assert!(matches!(normalized[1].event, Event::Key(k) if k.code == KeyCode::Enter));
}

#[test]
fn original_cursor_state_does_not_restart_blink_on_unchanged_paint() {
    use upstream_cursor::{CursorAction, CursorState};
    let mut cursor = CursorState::new();
    let mut wire = Vec::new();
    cursor.apply(cursor.action(Some((6, 25)), true), &mut wire);
    assert!(wire.ends_with(b"\x1b[?25h"));
    wire.clear();
    assert_eq!(cursor.action(Some((6, 25)), false), CursorAction::None);
    cursor.apply(cursor.action(Some((6, 25)), false), &mut wire);
    assert!(wire.is_empty());
    assert_eq!(cursor.action(None, false), CursorAction::Hide);
}
