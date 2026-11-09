use super::*;
fn enter() -> event::KeyEvent {
    event::KeyEvent::new_with_kind_and_state(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Press,
        event::KeyEventState::CSI_U_ENTER,
    )
}
fn history(start: usize, count: usize) -> Vec<Message> {
    (start..start + count)
        .map(|i| Message {
            id: format!("{i:06}"),
            role: "dot".into(),
            text: format!("MESSAGE-{i:03}\n{}", "中文 rich **history**\n".repeat(5)),
            files: vec![],
            created_at: "2026-11-04T06:29:00Z".into(),
        })
        .collect()
}
fn page_app() -> App {
    let mut a = App::demo();
    a.messages = history(100, 32);
    a.older_exhausted = false;
    a.follow_latest = false;
    a
}
#[test]
fn continuous_wheel_on_newly_ready_prepend_restores_anchor_before_scroll() {
    let (tx, _) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    for first in [68, 36, 4] {
        ui::settle(&a, area);
        a.scroll = 0;
        a.follow_latest = false;
        a.reading_anchor = None;
        let before = ui::reading_anchor(&a).unwrap();
        a.apply(Reply::Page(0, true, Ok(history(first, 32))), &tx);
        // Reproduce the scheduling edge: new layout is ready before the next wheel
        // flush, while the numeric scroll offset is still the old page's zero.
        ui::settle(&a, area);
        let restored = ui::resolve_anchor(&a, area, &before).unwrap();
        assert!(restored > 2);
        apply_scroll(&mut a, -2, area);
        assert_eq!(a.scroll, restored - 2);
        assert!(a.scroll > 0);
        assert!(!a.follow_latest);
    }
}
#[test]
fn ready_layout_never_paints_new_top_before_anchor_commit() {
    fn body(a: &App, area: Rect) -> Vec<String> {
        let mut t =
            Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height)).unwrap();
        t.draw(|f| ui::draw(f, a)).unwrap();
        let r = ui::history_rect(a, area);
        (r.y..r.bottom())
            .map(|y| {
                (r.x..r.right())
                    .map(|x| t.backend().buffer()[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }
    let (tx, _) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    a.scroll = 7;
    let before = body(&a, area);
    a.apply(Reply::Page(0, true, Ok(history(68, 32))), &tx);
    ui::settle(&a, area);
    assert!(a.reading_anchor.is_some());
    assert_eq!(
        body(&a, area),
        before,
        "paint must resolve its own ready snapshot before the next mutating settle"
    );
}
#[test]
fn pending_anchor_blocks_another_page_and_late_page_cannot_undo_return_to_bottom() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    a.scroll = 0;
    a.apply(Reply::Page(0, true, Ok(history(68, 32))), &tx);
    assert!(a.reading_anchor.is_some());
    request(&mut a, &tx, true);
    assert!(rx.try_recv().is_err());
    a.goto_bottom();
    a.apply(Reply::Page(0, true, Ok(history(36, 32))), &tx);
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    assert!(a.follow_latest);
    assert!(a.reading_anchor.is_none());
    assert_eq!(a.scroll, ui::scroll_limit(&a, area));
    let old = a.messages.clone();
    a.apply(Reply::Page(999, true, Ok(history(4, 32))), &tx);
    assert_eq!(a.messages, old);
}
#[test]
fn resize_keeps_message_anchor_and_compact_status_is_one_row_with_centered_click() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    let old = Rect::new(0, 0, 100, 30);
    ui::settle(&a, old);
    a.scroll = 20;
    let before = ui::reading_anchor(&a).unwrap();
    let new = Rect::new(0, 0, 72, 24);
    dispatch_input(&mut a, Event::Resize(72, 24), old, new, &tx);
    ui::settle(&a, new);
    sync_history_position(&mut a, new);
    assert_eq!(ui::reading_anchor(&a).unwrap().id, before.id);
    a.ui_visible = true;
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(72, 24)).unwrap();
    terminal.draw(|f| draw(f, &a)).unwrap();
    let status = ui::status_rect(&a, new);
    let hit = a.bottom_hit.get().unwrap();
    assert_eq!(status.height, 1);
    assert_eq!(hit.y, status.y);
    assert_eq!(hit.x + 1, status.x + status.width / 2);
    assert_eq!(ui::timeline(&a, new).content.bottom(), status.y);
    assert_eq!(status.bottom(), ui::prompt_hit_area(&a, new).y);
    assert_eq!(
        terminal.backend().buffer()[(hit.x + 1, hit.y)].symbol(),
        "▼"
    );
    mouse(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x + 1,
            row: hit.y,
            modifiers: KeyModifiers::NONE,
        },
        new,
        &tx,
    );
    sync_history_position(&mut a, new);
    assert!(a.follow_latest);
    assert_eq!(a.scroll, ui::scroll_limit(&a, new));
    assert!(rx.try_recv().is_err());
    terminal.draw(|f| draw(f, &a)).unwrap();
    assert!(a.bottom_hit.get().is_none());
}
#[test]
fn centered_bottom_arrow_does_not_take_hidden_modal_or_stale_layout_clicks() {
    let (tx, _) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    a.scroll = 2;
    let mut t = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    t.draw(|f| draw(f, &a)).unwrap();
    let hit = a.bottom_hit.get().unwrap();
    a.quit_review = true;
    mouse(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x + 1,
            row: hit.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &tx,
    );
    assert!(!a.follow_latest);
    a.quit_review = false;
    a.append("\n\n\n");
    mouse(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x + 1,
            row: hit.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &tx,
    );
    assert!(!a.follow_latest);
}
#[test]
fn bare_enter_submits_once_with_uuid_and_success_clears_only_accepted_draft() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("中文 once");
    key(&mut a, enter(), &tx);
    key(&mut a, enter(), &tx);
    let Job::Send(g, plan) = rx.try_recv().unwrap() else {
        panic!()
    };
    assert!(uuid::Uuid::parse_str(&plan.key).is_ok());
    assert_eq!(plan.text, "中文 once");
    assert_eq!(plan.dot.id, "demo");
    assert!(rx.try_recv().is_err());
    a.apply(Reply::Sent(g, plan.key, Ok("accepted".into())), &tx);
    assert_eq!(a.draft(), "");
    assert!(matches!(rx.try_recv(), Ok(Job::Page(..))));
}
#[test]
fn original_router_handles_shift_alt_backslash_and_modifier_rescue() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("中文");
    for modifier in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
        key(&mut a, event::KeyEvent::new(KeyCode::Enter, modifier), &tx);
    }
    assert_eq!(a.draft(), "中文\n\n");
    assert!(rx.try_recv().is_err());
    a.append("\\");
    key(&mut a, enter(), &tx);
    assert_eq!(a.draft(), "中文\n\n\n");
    assert_eq!(
        upstream_enter::route(&mut a.draft, &enter(), true),
        upstream_enter::EnterOutcome::NewlineInserted
    );
    assert!(rx.try_recv().is_err());
}
#[test]
fn repeat_release_and_rejected_double_press_require_explicit_history_recall() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("once");
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut k = enter();
        k.kind = kind;
        key(&mut a, k, &tx);
    }
    assert!(rx.try_recv().is_err());
    key(&mut a, enter(), &tx);
    let Job::Send(g, plan) = rx.try_recv().unwrap() else {
        panic!()
    };
    a.apply(
        Reply::Sent(
            g,
            plan.key,
            Err(SendError::Rejected("offline reject".into())),
        ),
        &tx,
    );
    for _ in 0..5 {
        key(&mut a, enter(), &tx);
    }
    assert!(rx.try_recv().is_err());
    assert_eq!(a.draft(), "");
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        &tx,
    );
    assert_eq!(a.draft(), "once");
    key(&mut a, enter(), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
}
#[test]
fn paste_and_same_batch_backlog_enter_never_submit() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    dispatch_input(&mut a, Event::Paste("中文\n多行".into()), area, area, &tx);
    a.last_paste = Some(Instant::now() - Duration::from_secs(1));
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    assert_eq!(a.draft(), "中文\n多行");
    a.paste_in_batch = false;
    key(&mut a, enter(), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
    let normalized = upstream_input::normalize_batch(vec![
        upstream_input::TimedInputEvent::now(Event::Paste("literal\n".into())),
        upstream_input::TimedInputEvent::now(Event::Key(enter())),
    ]);
    let mut fresh = App::demo();
    fresh.paste_in_batch = normalized
        .iter()
        .any(|e| matches!(e.event, Event::Paste(_)));
    for event in normalized {
        dispatch_input(&mut fresh, event.event, area, area, &tx);
    }
    assert!(rx.try_recv().is_err());
}
#[test]
fn send_guards_reject_missing_ambiguous_unbound_empty_readonly_hidden() {
    let (tx, rx) = mpsc::channel();
    for scenario in [0, 1, 2, 3, 4, 5, 6, 9, 10] {
        let mut a = App::demo();
        a.append("guard");
        match scenario {
            0 => a.dots.clear(),
            1 => a.dots.push(demo_dot()),
            2 => a.dots[0].room = None,
            3 => a.bound_target = None,
            4 => a.dots[0].room = Some("other".into()),
            5 => a.draft.set_text(" \n"),
            6 => a.enable_send = false,
            7 => a.busy = true,
            8 => a.uncertain = true,
            9 => a.ui_visible = false,
            _ => a.worker_failed = true,
        };
        key(&mut a, enter(), &tx);
        assert!(rx.try_recv().is_err(), "{scenario}");
    }
    let mut a = App::new();
    a.append("waiting");
    key(&mut a, enter(), &tx);
    assert_eq!(a.draft(), "waiting");
    assert!(rx.try_recv().is_err());
}
#[test]
fn resize_mismatch_and_narrow_screen_block_enter() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("visible");
    let area = Rect::new(0, 0, 100, 30);
    dispatch_input(
        &mut a,
        Event::Key(enter()),
        area,
        Rect::new(0, 0, 90, 30),
        &tx,
    );
    assert!(rx.try_recv().is_err());
    a.ui_visible = true;
    dispatch_input(
        &mut a,
        Event::Key(enter()),
        area,
        Rect::new(0, 0, 20, 5),
        &tx,
    );
    assert!(rx.try_recv().is_err());
}
#[test]
fn unknown_result_retains_history_but_not_a_permanent_input_lock() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("keep");
    key(&mut a, enter(), &tx);
    let Job::Send(g, p) = rx.try_recv().unwrap() else {
        panic!()
    };
    a.apply(
        Reply::Sent(g, p.key, Err(SendError::Uncertain("unknown".into()))),
        &tx,
    );
    assert_eq!(a.draft(), "");
    assert!(!a.submitted_history.is_empty());
    a.apply(Reply::Page(g, false, Ok(history(100, 32))), &tx);
    assert!(a.uncertain);
    a.append("new explicit message");
    key(&mut a, enter(), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
}
#[test]
fn latest_page_opens_at_bottom_and_refresh_does_not_steal_reading_position() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    a.follow_latest = true;
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    assert_eq!(a.scroll, ui::scroll_limit(&a, area));
    a.follow_latest = false;
    a.scroll = 7;
    let anchor = ui::reading_anchor(&a).unwrap();
    request(&mut a, &tx, false);
    rx.try_recv().unwrap();
    a.apply(Reply::Page(0, false, Ok(history(100, 32))), &tx);
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    let after = ui::reading_anchor(&a).unwrap();
    assert_eq!((anchor.id, anchor.row), (after.id, after.row));
    assert!(!a.follow_latest);
}
#[test]
fn top_further_wheel_loads_once_and_prepend_keeps_message_visual_row() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    a.scroll = 0;
    let before = ui::reading_anchor(&a).unwrap();
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    for _ in 0..12 {
        mouse(&mut a, wheel, area, &tx);
    }
    assert!(matches!(rx.try_recv(), Ok(Job::Page(_, _, Some(_)))));
    assert!(rx.try_recv().is_err());
    a.apply(Reply::Page(0, true, Ok(history(68, 32))), &tx);
    assert!(a.reading_anchor.is_some());
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    let after = ui::reading_anchor(&a).unwrap();
    assert_eq!((before.id, before.row), (after.id, after.row));
    assert!(a.scroll > 0 && !a.follow_latest);
}
#[test]
fn non_top_wheel_does_not_page_and_failed_page_needs_explicit_retry() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    a.scroll = 10;
    mouse(&mut a, wheel, area, &tx);
    assert!(rx.try_recv().is_err());
    a.scroll = 0;
    mouse(&mut a, wheel, area, &tx);
    rx.try_recv().unwrap();
    a.apply(Reply::Page(0, true, Err("offline failure".into())), &tx);
    for _ in 0..20 {
        mouse(&mut a, wheel, area, &tx);
    }
    assert!(rx.try_recv().is_err());
    assert!(a.status.contains("失败"));
    request(&mut a, &tx, true);
    assert!(rx.try_recv().is_ok());
}
#[test]
fn legacy_pages_do_not_claim_cursor_exhaustion_or_truncate_history() {
    let (tx, _rx) = mpsc::channel();
    for page in [Vec::new(), history(100, 32)] {
        let mut a = page_app();
        let area = Rect::new(0, 0, 100, 30);
        ui::settle(&a, area);
        a.apply(Reply::Page(0, true, Ok(page)), &tx);
        assert!(!a.older_exhausted);
        assert_eq!(a.messages.len(), 32);
        assert!(a.status.contains("当前分页"));
    }
    let mut a = page_app();
    a.messages = history(100, 280);
    a.apply(Reply::Page(0, true, Ok(history(68, 32))), &tx);
    assert_eq!(a.messages.len(), 312);
    assert!(!a.older_exhausted);
}
#[test]
fn anchor_survives_reflow_and_duplicate_message_ids_remain_unique() {
    let (tx, _) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    ui::settle(&a, area);
    a.scroll = 8;
    let before = ui::reading_anchor(&a).unwrap();
    a.apply(Reply::Page(0, true, Ok(history(84, 32))), &tx);
    let resized = Rect::new(0, 0, 70, 24);
    ui::settle(&a, resized);
    sync_history_position(&mut a, resized);
    let after = ui::reading_anchor(&a).unwrap();
    assert_eq!((before.id, before.row), (after.id, after.row));
    let ids: std::collections::HashSet<_> = a.messages.iter().map(|m| &m.id).collect();
    assert_eq!(ids.len(), a.messages.len());
    assert_eq!(a.messages.len(), 48);
}

fn paint(a: &App, area: Rect) -> Terminal<ratatui::backend::TestBackend> {
    ui::settle(a, area);
    let mut t = Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height)).unwrap();
    t.draw(|f| draw(f, a)).unwrap();
    t
}
fn pointer(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}
fn click(a: &mut App, area: Rect, tx: &mpsc::Sender<Job>, x: u16, y: u16) {
    mouse(
        a,
        pointer(MouseEventKind::Down(MouseButton::Left), x, y),
        area,
        tx,
    );
    mouse(
        a,
        pointer(MouseEventKind::Up(MouseButton::Left), x, y),
        area,
        tx,
    );
}
#[test]
fn click_focus_and_space_consume_transition_and_unfocused_enter_never_sends() {
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    paint(&a, area);
    a.append("keep");
    click(&mut a, area, &tx, 10, 8);
    assert!(!a.prompt_focused);
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    let t = paint(&a, area);
    assert!(ui::cursor_position(&a, area).is_none());
    let text: String = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        text.replace(' ', "")
            .contains(&i18n::label("Space:prompt", "Space:输入").replace(' ', ""))
    );
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        &tx,
    );
    assert!(a.prompt_focused);
    assert_eq!(a.draft(), "keep");
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err(), "same focus batch cannot submit");
    a.focus_in_batch = false;
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        &tx,
    );
    assert_eq!(a.draft(), "keep ");
    let composer = ui::prompt_hit_area(&a, area);
    click(&mut a, area, &tx, composer.x + 4, composer.y + 1);
    assert!(a.prompt_focused);
}
#[test]
fn focus_batch_boundary_modal_narrow_rail_and_buttons_have_no_unintended_send() {
    let (tx, rx) = mpsc::channel();
    let mut a = page_app();
    let area = Rect::new(0, 0, 100, 30);
    paint(&a, area);
    a.append("draft");
    let track = ui::timeline(&a, area).track.unwrap();
    click(&mut a, area, &tx, track.x, track.y);
    assert!(!a.prompt_focused);
    assert_eq!(
        dispatch_input(
            &mut a,
            Event::Key(event::KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
            area,
            area,
            &tx
        ),
        InputFlow::Paint
    );
    dispatch_input(&mut a, Event::Key(enter()), area, area, &tx);
    assert!(rx.try_recv().is_err());
    a.quit_review = true;
    let was = a.prompt_focused;
    click(&mut a, area, &tx, 8, 25);
    assert_eq!(a.prompt_focused, was);
    a.quit_review = false;
    a.ui_visible = false;
    key(
        &mut a,
        event::KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        &tx,
    );
    assert!(rx.try_recv().is_err());
}
#[test]
fn unfocused_empty_placeholder_and_cursor_follow_original_focus_rule() {
    let (tx, _) = mpsc::channel();
    let mut a = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    paint(&a, area);
    click(&mut a, area, &tx, 10, 8);
    let t = paint(&a, area);
    let text: String = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(a.draft().is_empty());
    assert!(!text.contains("给 dot 发消息"));
    assert!(ui::cursor_position(&a, area).is_none());
    let prompt = ui::prompt_hit_area(&a, area);
    click(&mut a, area, &tx, prompt.x + 4, prompt.y + 1);
    let t = paint(&a, area);
    let text: String = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(a.draft().is_empty());
    assert!(!text.contains("给 dot 发消息"));
    assert!(ui::cursor_position(&a, area).is_some());
}
fn fold_app() -> App {
    let mut a = App::demo();
    a.messages = vec![
        Message {
            id: "user-long".into(),
            role: "you".into(),
            text: "first\nsecond\nthird\nfourth\nfifth\nsixth".into(),
            files: vec![],
            created_at: String::new(),
        },
        Message {
            id: "assistant-long".into(),
            role: "dot".into(),
            text: "assistant\n".repeat(50),
            files: vec![],
            created_at: String::new(),
        },
    ];
    a.follow_latest = false;
    a.scroll = 0;
    a
}
#[test]
fn original_user_default_three_lines_and_double_click_toggle_survive_refresh() {
    let (tx, _) = mpsc::channel();
    let mut a = fold_app();
    let area = Rect::new(0, 0, 100, 30);
    paint(&a, area);
    let short = ui::timeline(&a, area).total;
    let t = paint(&a, area);
    let text: String = t
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("third") && text.contains('…') && !text.contains("fourth"));
    click(&mut a, area, &tx, 7, 5);
    assert!(a.expanded_messages.is_empty());
    click(&mut a, area, &tx, 7, 5);
    assert_eq!(a.expanded_messages.get("user-long"), Some(&true));
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    assert!(ui::timeline(&a, area).total > short);
    let anchor = ui::reading_anchor(&a).unwrap();
    assert_eq!((anchor.id, anchor.row), ("user-long".into(), 0));
    a.apply(Reply::Page(0, false, Ok(a.messages.clone())), &tx);
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    assert_eq!(a.expanded_messages.get("user-long"), Some(&true));
    // Third click in a sequence is not another toggle for a user prompt.
    click(&mut a, area, &tx, 7, 5);
    assert_eq!(a.expanded_messages.get("user-long"), Some(&true));
    a.last_history_click = None;
    click(&mut a, area, &tx, 7, 5);
    click(&mut a, area, &tx, 7, 5);
    ui::settle(&a, area);
    sync_history_position(&mut a, area);
    assert_eq!(a.expanded_messages.get("user-long"), Some(&false));
    assert_eq!(ui::timeline(&a, area).total, short);
}
#[test]
fn assistant_short_prompt_word_select_and_drag_do_not_fold() {
    let (tx, _) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    for mode in 0..4 {
        let mut a = fold_app();
        if mode == 0 {
            a.messages[0].role = "dot".into();
        }
        if mode == 1 {
            a.messages[0].text = "short\nthree\nrows".into();
        }
        if mode == 2 {
            a.text_selection = upstream_text_selection::TextSelection::WordSelect;
        }
        paint(&a, area);
        if mode == 3 {
            for _ in 0..2 {
                mouse(
                    &mut a,
                    pointer(MouseEventKind::Down(MouseButton::Left), 7, 5),
                    area,
                    &tx,
                );
                mouse(
                    &mut a,
                    pointer(MouseEventKind::Drag(MouseButton::Left), 8, 5),
                    area,
                    &tx,
                );
                mouse(
                    &mut a,
                    pointer(MouseEventKind::Up(MouseButton::Left), 8, 5),
                    area,
                    &tx,
                );
            }
        } else {
            click(&mut a, area, &tx, 7, 5);
            click(&mut a, area, &tx, 7, 5);
        }
        assert!(a.expanded_messages.is_empty(), "{mode}");
    }
}
#[test]
fn upstream_click_window_same_entry_and_drag_threshold_are_exact() {
    let now = Instant::now();
    assert_eq!(
        upstream_user_fold::click_count(Some((now, "a", 1)), now + Duration::from_millis(299), "a"),
        2
    );
    assert_eq!(
        upstream_user_fold::click_count(Some((now, "a", 1)), now + Duration::from_millis(300), "a"),
        1
    );
    assert_eq!(
        upstream_user_fold::click_count(Some((now, "a", 1)), now + Duration::from_millis(10), "b"),
        1
    );
    assert!(upstream_user_fold::drag_threshold_exceeded((7, 5), 8, 5));
    assert!(!upstream_user_fold::drag_threshold_exceeded((7, 5), 7, 5));
}
#[test]
fn outer_scrollbar_defaults_keep_content_padding_and_render_hit_geometry() {
    for area in [Rect::new(0, 0, 100, 30), Rect::new(7, 5, 80, 24)] {
        let a = page_app();
        ui::settle(&a, area);
        let timeline = ui::timeline(&a, area);
        let track = timeline.track.unwrap();
        assert_eq!(track.x, area.right() - 1);
        assert_eq!(timeline.content.x, area.x + 2);
        assert_eq!(timeline.content.right(), area.right() - 2);
        assert_eq!(track.width, 1);
    }
}
