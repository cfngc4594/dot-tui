use super::*;
fn pointer(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}
#[test]
fn editing_is_grapheme_aware_and_never_modifies_history_or_sends() {
    let mut app = App::demo();
    let history = app.messages.clone();
    let (tx, rx) = mpsc::channel();
    app.append("A中e\u{301}👩‍💻Z");
    for code in [KeyCode::Left, KeyCode::Left, KeyCode::Backspace] {
        key(
            &mut app,
            event::KeyEvent::new(code, KeyModifiers::NONE),
            &tx,
        );
    }
    assert_eq!(app.draft(), "A中👩‍💻Z");
    app.append("新");
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
        &tx,
    );
    assert_eq!(app.draft(), "A中新Z");
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
        &tx,
    );
    app.append("头\n");
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
        &tx,
    );
    app.append("尾");
    assert_eq!(app.draft(), "头\nA中新Z尾");
    assert_eq!(app.messages, history);
    assert!(rx.try_recv().is_err());
    app.enable_send = false;
    let draft = app.draft().to_owned();
    app.edit(KeyCode::Delete);
    app.append("ignored");
    assert_eq!(app.draft(), draft);
}
#[test]
fn click_drag_release_resize_and_wheel_share_live_scroll_state() {
    let mut app = App::demo();
    app.messages[0].text = "long row\n".repeat(200);
    let (tx, _) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    let geometry = ui::timeline(&app, area);
    let track = geometry.track.unwrap();
    mouse(
        &mut app,
        pointer(
            MouseEventKind::Down(MouseButton::Left),
            track.x - 1,
            track.y,
        ),
        area,
        &tx,
    );
    assert_eq!(app.scroll, 0);
    assert!(!app.follow_latest);
    let mid = track.y + track.height / 2;
    mouse(
        &mut app,
        pointer(MouseEventKind::Drag(MouseButton::Left), 0, mid),
        area,
        &tx,
    );
    assert!(app.scroll > 0 && app.scroll < geometry.limit());
    mouse(
        &mut app,
        pointer(MouseEventKind::Drag(MouseButton::Left), 0, u16::MAX),
        area,
        &tx,
    );
    assert_eq!(app.scroll, geometry.limit());
    assert!(app.follow_latest);
    mouse(
        &mut app,
        pointer(MouseEventKind::Up(MouseButton::Left), 0, 0),
        area,
        &tx,
    );
    assert!(app.scrollbar_drag.is_none());
    mouse(
        &mut app,
        pointer(MouseEventKind::Drag(MouseButton::Left), track.x, track.y),
        area,
        &tx,
    );
    assert_eq!(
        app.scroll,
        geometry.limit(),
        "release outside ends dragging"
    );
    mouse(
        &mut app,
        pointer(MouseEventKind::Down(MouseButton::Left), track.x, mid),
        area,
        &tx,
    );
    let offset = app.scroll;
    mouse(
        &mut app,
        pointer(MouseEventKind::Drag(MouseButton::Left), track.x, track.y),
        Rect::new(0, 0, 80, 25),
        &tx,
    );
    assert_eq!(app.scroll, offset);
    assert!(app.scrollbar_drag.is_none());
    apply_scroll(&mut app, -100000, area);
    assert_eq!(app.scroll, 0);
    apply_scroll(&mut app, 100000, area);
    assert_eq!(app.scroll, geometry.limit());
    mouse(
        &mut app,
        pointer(MouseEventKind::ScrollUp, 10, 10),
        area,
        &tx,
    );
    assert!(!app.follow_latest);
    let delta = app
        .scroller
        .flush(Instant::now() + Duration::from_millis(120));
    apply_scroll(&mut app, delta, area);
    assert!(app.scroll < geometry.limit());
}
#[test]
fn modal_and_narrow_screen_never_grab_hidden_scrollbar_or_dispatch_jobs() {
    let mut app = App::demo();
    app.messages[0].text = "row\n".repeat(200);
    let (tx, rx) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    let track = ui::timeline(&app, area).track.unwrap();
    app.append("review");
    app.review_send();
    mouse(
        &mut app,
        pointer(MouseEventKind::Down(MouseButton::Left), track.x, track.y),
        area,
        &tx,
    );
    assert!(app.scrollbar_drag.is_none());
    assert!(rx.try_recv().is_err());
    app.review = None;
    app.ui_visible = false;
    mouse(
        &mut app,
        pointer(MouseEventKind::Down(MouseButton::Left), track.x, track.y),
        Rect::new(0, 0, 20, 5),
        &tx,
    );
    assert!(app.scrollbar_drag.is_none());
    assert!(rx.try_recv().is_err());
}

#[test]
fn scrollbar_click_inverse_places_the_rendered_thumb_on_the_clicked_row() {
    let mut app = App::demo();
    app.messages[0].text = "row\n".repeat(400);
    let (tx, _) = mpsc::channel();
    let area = Rect::new(0, 0, 60, 22);
    let track = ui::timeline(&app, area).track.unwrap();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(60, 22)).unwrap();
    for y in track.y..track.bottom() {
        mouse(
            &mut app,
            pointer(MouseEventKind::Down(MouseButton::Left), track.x, y),
            area,
            &tx,
        );
        terminal.draw(|f| draw(f, &app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(track.x, y)].symbol(),
            "█",
            "row {y}"
        );
        mouse(
            &mut app,
            pointer(MouseEventKind::Up(MouseButton::Left), 0, 0),
            area,
            &tx,
        );
    }
}

#[test]
fn document_home_end_and_hidden_wheel_preserve_expected_state() {
    let mut app = App::demo();
    let (tx, _) = mpsc::channel();
    app.append("first\n中e\u{301}\nlast");
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL),
        &tx,
    );
    assert_eq!(app.draft_cursor(), 0);
    app.append("start ");
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL),
        &tx,
    );
    assert_eq!(app.draft_cursor(), app.draft().len());
    app.messages[0].text = "row\n".repeat(100);
    app.scroll = 8;
    app.follow_latest = false;
    app.ui_visible = false;
    mouse(
        &mut app,
        pointer(MouseEventKind::ScrollDown, 1, 1),
        Rect::new(0, 0, 20, 5),
        &tx,
    );
    assert_eq!(app.scroll, 8);
    assert!(!app.follow_latest);
}
