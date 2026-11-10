//! Real app rendering/graphics routing with synthetic media and no account I/O.
use super::*;

fn scene(file: &str) -> App {
    inline_history::clear();
    let mut app = App::demo();
    app.messages = vec![
        Message {
            id: "focus-image".into(),
            role: "dot".into(),
            text: "Visible history image".into(),
            files: vec![file.into()],
            created_at: String::new(),
        },
        Message {
            id: "after-image".into(),
            role: "dot".into(),
            text: "Later history row\n\n".repeat(70),
            files: vec![],
            created_at: String::new(),
        },
    ];
    app.follow_latest = false;
    app.scroll = 0;
    let (tx, _) = mpsc::channel();
    app.apply(
        Reply::HistoryImage(
            "demo-room".into(),
            file.into(),
            native_media::prepare(demo_image_view(&demo_dot(), file).unwrap()),
        ),
        &tx,
    );
    app
}

fn paint(app: &App, area: Rect) -> String {
    ui::settle(app, area);
    let mut terminal =
        Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let mut out = paint_inline_history(app, area);
    for _ in 0..256 {
        if !inline_history::upload_pending() {
            return out;
        }
        out.push_str(&paint_inline_history(app, area));
    }
    panic!("synthetic media upload did not finish");
}

#[test]
fn focus_changes_preserve_history_placements_without_retransmission() {
    let _protocol =
        terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let area = Rect::new(0, 0, 128, 40);
    for file in ["fixture-0", "fixture-1"] {
        let mut app = scene(file);
        let (tx, rx) = mpsc::channel();
        let first = paint(&app, area);
        assert!(first.contains("a=t") && first.contains("a=p"));
        let draft = app.draft().to_owned();
        for event in [
            Event::FocusLost,
            Event::FocusLost,
            Event::FocusGained,
            Event::FocusLost,
            Event::FocusGained,
        ] {
            dispatch_input(&mut app, event, area, area, &tx);
            let next = paint(&app, area);
            assert!(!app.history_frame.borrow().is_empty());
            assert!(!next.contains("a=d"), "focus alone must not delete pixels");
            assert!(
                !next.contains("a=t"),
                "focus alone must not reupload pixels"
            );
            if !app.window_focused {
                dispatch_input(&mut app, Event::Paste("ignored".into()), area, area, &tx);
                key(
                    &mut app,
                    event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    &tx,
                );
                assert!(ui::cursor_position(&app, area).is_none());
            }
            assert_eq!(app.draft(), draft);
            assert!(
                rx.try_recv().is_err(),
                "focus changes must not send or fetch"
            );
        }
        inline_history::clear();
    }
}

#[test]
fn background_history_still_crops_clears_offscreen_resizes_and_obeys_occlusion() {
    let _protocol =
        terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let mut app = scene("fixture-0");
    let area = Rect::new(0, 0, 100, 30);
    let (tx, _) = mpsc::channel();
    dispatch_input(&mut app, Event::FocusLost, area, area, &tx);
    let first = paint(&app, area);
    assert!(
        first.contains("a=t") && first.contains("a=p"),
        "background completion must paint"
    );
    let placement = app.history_frame.borrow()[0].clone();
    app.scroll = usize::from(placement.area.y - ui::history_rect(&app, area).y + 2);
    let cropped = paint(&app, area);
    assert!(app.history_frame.borrow()[0].top_crop > 0);
    assert!(cropped.contains("a=d") && cropped.contains("a=p"));
    assert!(!cropped.contains("a=t"));
    app.scroll = 200;
    let gone = paint(&app, area);
    assert!(app.history_frame.borrow().is_empty());
    assert!(gone.contains("a=d") && !gone.contains("a=p"));
    app.scroll = 0;
    assert!(paint(&app, area).contains("a=p"));
    let resized = paint(&app, Rect::new(0, 0, 90, 32));
    assert!(resized.contains("a=d") && resized.contains("a=p"));
    let tiny = paint(&app, Rect::new(0, 0, 20, 8));
    assert!(tiny.contains("a=d") && !tiny.contains("a=p"));
    assert!(paint(&app, area).contains("a=p"));
    app.quit_review = true;
    let modal = paint(&app, area);
    assert!(modal.contains("a=d") && !modal.contains("a=p"));
    app.quit_review = false;
    assert!(paint(&app, area).contains("a=p"));
    app.picker.context = file_picker::context::detect("@image", 6);
    assert!(app.picker.context.is_some());
    let dropdown = paint(&app, area);
    assert!(dropdown.contains("a=d") && !dropdown.contains("a=p"));
    app.picker.context = None;
    assert!(paint(&app, area).contains("a=p"));
    assert!(inline_history::clear().contains("a=d"));
}

#[test]
fn unfocused_unsupported_terminal_keeps_compact_fallback_without_graphics() {
    let _protocol = terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::None);
    let mut app = scene("fixture-0");
    app.window_focused = false;
    let out = paint(&app, Rect::new(0, 0, 128, 40));
    assert!(app.history_frame.borrow().is_empty());
    assert!(!out.contains("a=t") && !out.contains("a=p"));
    assert!(inline_history::clear().is_empty());
}
