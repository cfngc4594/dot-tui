//! App-level image tests: real input routing, worker staging, renderer flush
//! boundary, immutable SendPlan and fake service. No native clipboard or HTTP.
use super::*;
fn channels() -> (mpsc::Sender<Job>, mpsc::Receiver<Job>) {
    mpsc::channel()
}
fn enter() -> event::KeyEvent {
    let mut k = event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    k.state.insert(event::KeyEventState::CSI_U_ENTER);
    k
}
fn paint(app: &App, width: u16, height: u16, commit: bool) -> String {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    if commit {
        app.commit_image_preview();
    }
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn staged() -> (App, Source, mpsc::Sender<Job>, mpsc::Receiver<Job>) {
    let mut app = App::demo();
    let mut source = Source::Demo(FakeBackend::new());
    let (tx, rx) = channels();
    app.image_clipboard_fixture = Some(image_ingress::synthetic());
    assert!(!key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
        &tx
    ));
    assert_eq!(app.image_loading, 1);
    assert!(app.draft.images.is_empty());
    app.apply(source.perform(rx.recv().unwrap()), &tx);
    assert_eq!(app.draft.images.len(), 1);
    app.paste_in_batch = false;
    app.last_paste = None;
    (app, source, tx, rx)
}
#[test]
fn actual_app_image_only_preview_then_one_fake_upload_and_message() {
    let (mut a, mut source, tx, rx) = staged();
    assert!(paint(&a, 100, 30, false).contains("Image #1"));
    key(&mut a, enter(), &tx);
    assert!(
        rx.try_recv().is_err(),
        "rendering without committed flush must not authorize upload"
    );
    a.commit_image_preview();
    key(&mut a, enter(), &tx);
    key(&mut a, enter(), &tx);
    let job = rx.recv().unwrap();
    let Job::Send(_, p) = &job else {
        panic!("expected send")
    };
    assert_eq!(p.images.len(), 1);
    assert!(p.text.trim().is_empty());
    assert!(!p.text.contains("Image #"));
    assert!(rx.try_recv().is_err());
    a.apply(source.perform(job), &tx);
    let Source::Demo(b) = source else {
        unreachable!()
    };
    assert_eq!(b.image_uploads, 1);
    assert_eq!(b.sends, 1);
    assert!(a.draft.images.is_empty());
    assert!(a.draft().is_empty());
}
#[test]
fn image_guards_block_until_visible_and_explicit_bare_enter() {
    let (mut a, _, tx, rx) = staged();
    paint(&a, 100, 30, true);
    a.image_loading = 1;
    key(&mut a, enter(), &tx);
    a.image_loading = 0;
    a.bound_target = None;
    key(&mut a, enter(), &tx);
    a.bound_target = Some(("demo".into(), "demo-room".into()));
    a.ui_visible = false;
    paint(&a, 20, 8, true);
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    assert_eq!(a.draft.images.len(), 1);
    a.ui_visible = true;
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &tx,
    );
    assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
    assert!(a.draft.images.is_empty());
}
#[test]
fn stale_local_read_after_clear_cannot_reappear() {
    let mut a = App::demo();
    let (tx, rx) = channels();
    let mut source = Source::Demo(FakeBackend::new());
    a.stage_image(image_ingress::Input::Bytes(image_ingress::synthetic()), &tx);
    let job = rx.recv().unwrap();
    a.reset_draft();
    a.append("new draft");
    a.apply(source.perform(job), &tx);
    assert_eq!(a.draft(), "new draft");
    assert!(a.draft.images.is_empty());
    assert_eq!(a.image_loading, 0);
}
#[test]
fn failed_image_upload_retains_history_and_never_posts_message() {
    let (mut a, mut source, tx, rx) = staged();
    paint(&a, 100, 30, true);
    let Source::Demo(b) = &mut source else {
        unreachable!()
    };
    b.fail_uncertain = true;
    key(&mut a, enter(), &tx);
    let job = rx.recv().unwrap();
    let original_bytes = a.submitted_history.last().unwrap().images[0]
        .encoded_bytes
        .clone();
    assert!(a.draft.images.is_empty());
    a.apply(source.perform(job), &tx);
    assert!(!a.uncertain);
    assert!(
        a.send_error
            .as_ref()
            .unwrap()
            .contains("Message not submitted")
    );
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &tx,
    );
    assert_eq!(a.draft.images.len(), 1);
    assert_eq!(a.draft.images[0].encoded_bytes, original_bytes);
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &tx,
    );
    assert_eq!(a.draft.images.len(), 1);
    assert!(rx.try_recv().is_err());
    let Source::Demo(b) = source else {
        unreachable!()
    };
    assert_eq!(b.image_uploads, 1);
    assert_eq!(b.sends, 0);
}
#[test]
fn explicit_local_path_paste_uses_same_worker_and_preserves_file() {
    let path = std::env::temp_dir().join(format!("dot-app-paste-{}.png", uuid::Uuid::new_v4()));
    let bytes = image_ingress::synthetic();
    std::fs::write(&path, &bytes).unwrap();
    let mut a = App::demo();
    let mut source = Source::Demo(FakeBackend::new());
    let (tx, rx) = channels();
    let area = Rect::new(0, 0, 100, 30);
    dispatch_input(
        &mut a,
        Event::Paste(path.to_string_lossy().into_owned()),
        area,
        area,
        &tx,
    );
    a.apply(source.perform(rx.recv().unwrap()), &tx);
    assert_eq!(a.draft.images.len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn selector_enter_is_consumed_as_local_selection_and_same_batch_enter_cannot_send() {
    let path = std::env::temp_dir().join(format!("dot-app-picker-{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&path, image_ingress::synthetic()).unwrap();
    let mut a = App::demo();
    let (tx, rx) = channels();
    a.append("@");
    a.picker.update(a.draft.text(), a.draft.cursor());
    a.picker
        .accept_results("", vec![path.to_string_lossy().into_owned()]);
    paint(&a, 100, 30, true);
    key(&mut a, enter(), &tx);
    key(&mut a, enter(), &tx);
    assert!(matches!(rx.recv().unwrap(), Job::StageImage(..)));
    assert!(rx.try_recv().is_err());
    assert!(a.pending.is_none());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn received_attachment_click_runs_metadata_download_original_preview_without_send() {
    let mut a = App::demo();
    a.messages[0].files = vec!["fake-file-1".into()];
    a.history_changed();
    let area = Rect::new(0, 0, 100, 30);
    let mut hit = None;
    for _ in 0..100 {
        paint(&a, 100, 30, false);
        for y in 0..30 {
            for x in 0..100 {
                if ui::attachment_hit(&a, area, x, y).is_some() {
                    hit = Some((x, y));
                    break;
                }
            }
            if hit.is_some() {
                break;
            }
        }
        if hit.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let (x, y) = hit.expect("received attachment chip has a real hit area");
    let (tx, rx) = channels();
    let mut source = Source::Demo(FakeBackend::new());
    let scroll = a.scroll;
    mouse(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &tx,
    );
    assert!(a.image_view_loading);
    assert!(!a.can_edit());
    a.apply(source.perform(rx.recv().unwrap()), &tx);
    assert!(a.received_image.is_some());
    assert_eq!(a.native_media.viewer_requests, 1);
    assert!(paint(&a, 100, 30, true).contains("Esc close"));
    assert_eq!(a.scroll, scroll);
    key(&mut a, enter(), &tx);
    assert!(a.received_image.is_none());
    assert!(rx.try_recv().is_err());
    let Source::Demo(b) = source else {
        unreachable!()
    };
    assert_eq!(b.image_uploads, 0);
    assert_eq!(b.sends, 0);
}
#[test]
fn canceled_incoming_download_cannot_reopen_or_send_draft() {
    let mut a = App::demo();
    a.append("keep draft");
    let (tx, rx) = channels();
    let mut source = Source::Demo(FakeBackend::new());
    a.open_received_image("fake-file-1".into(), &tx);
    let job = rx.recv().unwrap();
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &tx,
    );
    a.apply(source.perform(job), &tx);
    assert!(a.received_image.is_none());
    assert_eq!(a.draft(), "keep draft");
    assert!(!a.image_view_loading);
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
}

#[test]
fn delayed_download_after_focus_loss_never_launches_native_viewer() {
    let mut a = App::demo();
    let (tx, rx) = channels();
    let mut source = Source::Demo(FakeBackend::new());
    a.open_received_image("fake-file-1".into(), &tx);
    a.window_focused = false;
    a.apply(source.perform(rx.recv().unwrap()), &tx);
    assert_eq!(a.native_media.viewer_requests, 0);
    assert!(a.received_image.is_none());
    a.window_focused = true;
    a.open_received_image("fake-file-1".into(), &tx);
    assert_eq!(a.native_media.viewer_requests, 1);
    assert!(rx.try_recv().is_err());
}
#[test]
fn repeated_pending_image_clicks_only_queue_one_download() {
    let mut a = App::demo();
    let (tx, rx) = channels();
    a.open_received_image("fake-file-1".into(), &tx);
    a.open_received_image("fake-file-1".into(), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::ViewImage(..))));
    assert!(rx.try_recv().is_err());
}
#[test]
fn prompt_image_overlay_stays_above_status_prompt_and_footer() {
    let (a, _, _, _) = staged();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    let mut plain = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    plain.draw(|f| ui::draw(f, &a)).unwrap();
    let mut preview = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    preview.draw(|f| draw(f, &a)).unwrap();
    assert!(a.image_painted.get().is_some());
    for y in ui::status_rect(&a, area).y..area.bottom() {
        for x in 0..100 {
            assert_eq!(
                plain.backend().buffer()[(x, y)],
                preview.backend().buffer()[(x, y)],
                "preview crossed into chrome at {x},{y}"
            );
        }
    }
}
#[test]
fn original_media_button_paint_and_hit_share_offset_width_and_style() {
    use ratatui::{
        style::{Color, Modifier},
        widgets::Paragraph,
    };
    for width in [28, 57, 96] {
        let area = Rect::new(9, 5, width, 8);
        let (lines, hit) = upstream_media::attachment_lines(0, width);
        let (col, w) = hit.unwrap();
        let mut b = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(Paragraph::new(lines), area, &mut b);
        assert_eq!(col, upstream_media::media_open_button_col(width, false));
        assert_eq!(w, 12);
        let cell = &b[(area.x + col, area.y + 3)];
        assert_eq!(cell.symbol(), "[");
        assert_eq!(cell.fg, Color::Rgb(58, 149, 171));
        assert!(cell.modifier.contains(Modifier::BOLD));
    }
    assert!(upstream_media::attachment_lines(0, 5).1.is_none());
}
#[test]
fn historical_visible_downloads_are_bounded_room_bound_and_never_send() {
    let _guard = terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let mut app = App::demo();
    app.ui_visible = true;
    app.visible_media
        .borrow_mut()
        .extend((0..6).map(|i| format!("file-{i}")));
    let (tx, rx) = channels();
    app.warm_visible_media(&tx);
    app.warm_visible_media(&tx);
    let jobs: Vec<_> = rx.try_iter().collect();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|j| matches!(j, Job::LoadHistoryImage(..))));
    app.apply(
        Reply::HistoryImage("wrong-room".into(), "file-1".into(), Err("late".into())),
        &tx,
    );
    assert!(app.native_media.cached("wrong-room/file-1").is_none());
}
#[test]
fn click_during_history_download_shares_result_and_cancel_prevents_late_open() {
    let mut app = App::demo();
    app.ui_visible = true;
    let (tx, rx) = channels();
    app.history_loading.insert("demo-room/fake-file-1".into());
    app.open_received_image("fake-file-1".into(), &tx);
    assert!(rx.try_recv().is_err());
    assert!(app.image_view_loading);
    key(
        &mut app,
        event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &tx,
    );
    let prepared =
        native_media::prepare(demo_image_view(&demo_dot(), "fake-file-1").unwrap()).unwrap();
    app.apply(
        Reply::HistoryImage("demo-room".into(), "fake-file-1".into(), Ok(prepared)),
        &tx,
    );
    assert_eq!(app.native_media.viewer_requests, 0);
    assert!(!app.image_view_loading);
    app.open_received_image("fake-file-1".into(), &tx);
    assert_eq!(app.native_media.viewer_requests, 1);
}
#[test]
fn explicit_download_is_not_duplicated_by_background_warmup() {
    let _guard = terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let mut app = App::demo();
    let (tx, rx) = channels();
    app.open_received_image("fake-file-1".into(), &tx);
    assert!(matches!(rx.recv().unwrap(), Job::ViewImage(..)));
    app.visible_media.borrow_mut().insert("fake-file-1".into());
    app.warm_visible_media(&tx);
    assert!(rx.try_recv().is_err());
}
#[test]
fn failed_automatic_image_has_compact_error_and_no_request_storm_until_explicit_retry() {
    let _protocol =
        terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let mut app = App::demo();
    let (tx, rx) = channels();
    app.visible_media.borrow_mut().insert("fake-file-1".into());
    app.warm_visible_media(&tx);
    assert!(matches!(rx.recv().unwrap(), Job::LoadHistoryImage(..)));
    app.apply(
        Reply::HistoryImage(
            "demo-room".into(),
            "fake-file-1".into(),
            Err("offline fixture failure".into()),
        ),
        &tx,
    );
    for _ in 0..20 {
        app.warm_visible_media(&tx);
    }
    assert!(rx.try_recv().is_err());
    let info = upstream_media::Info {
        error: None,
        name: "Image".into(),
        dimensions: (0, 0),
    };
    let (lines, _, rows) = upstream_media::history_lines("fake-file-1", Some(&info), 80, true);
    assert!(rows.is_none());
    assert_eq!(lines.len(), 2);
    assert!(
        !lines
            .iter()
            .any(|line| line.to_string().contains("unavailable"))
    );
    assert!(lines[1].to_string().contains("Open"));
    app.open_received_image("fake-file-1".into(), &tx);
    let mut source = Source::Demo(FakeBackend::new());
    let reply = source.perform(rx.recv().unwrap());
    app.apply(reply, &tx);
    assert!(app.history_failed.is_empty());
    assert_eq!(app.native_media.viewer_requests, 1);
}
#[test]
fn app_does_not_reload_capacity_denied_same_screen_images_and_recovers_on_reentry() {
    let _protocol =
        terminal::image::set_protocol_for_test(terminal::image::GraphicsProtocol::Kitty);
    let mut a = App::demo();
    a.native_media = native_media::Store::with_test_limits(native_media::MAX_RESIDENT, 1);
    a.visible_media
        .borrow_mut()
        .extend(["fake-file-1".to_owned(), "fake-file-2".to_owned()]);
    let (tx, rx) = channels();
    a.warm_visible_media(&tx);
    let jobs: Vec<_> = rx.try_iter().collect();
    assert_eq!(jobs.len(), 2);
    for job in jobs {
        let Job::LoadHistoryImage(_, file) = job else {
            unreachable!()
        };
        let prepared = native_media::prepare(demo_image_view(&demo_dot(), &file).unwrap()).unwrap();
        a.apply(
            Reply::HistoryImage("demo-room".into(), file, Ok(prepared)),
            &tx,
        );
    }
    for _ in 0..100 {
        a.warm_visible_media(&tx);
    }
    assert!(rx.try_recv().is_err());
    let denied = ["fake-file-1", "fake-file-2"]
        .into_iter()
        .find(|f| a.native_media.cached(&format!("demo-room/{f}")).is_none())
        .unwrap();
    a.visible_media.borrow_mut().clear();
    a.warm_visible_media(&tx);
    a.visible_media.borrow_mut().insert(denied.to_owned());
    a.warm_visible_media(&tx);
    assert!(matches!(rx.recv().unwrap(),Job::LoadHistoryImage(_,f) if f==denied));
    assert!(rx.try_recv().is_err());
}
