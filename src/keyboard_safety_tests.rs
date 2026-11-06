use super::*;
fn encoded(modifiers: KeyModifiers) -> event::KeyEvent {
    event::KeyEvent::new_with_kind_and_state(
        KeyCode::Enter,
        modifiers,
        KeyEventKind::Press,
        event::KeyEventState::CSI_U_ENTER,
    )
}
#[test]
fn unknown_protocol_or_raw_cr_never_submits_even_if_session_confirmed() {
    let (tx, rx) = mpsc::channel();
    for confirmed in [false, true] {
        for is_encoded in [false, true] {
            let mut a = App::demo();
            a.keyboard_confirmed = confirmed;
            a.append("safe");
            let k = if is_encoded {
                encoded(KeyModifiers::NONE)
            } else {
                event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            };
            key(&mut a, k, &tx);
            if confirmed && is_encoded {
                assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
            } else {
                assert!(rx.try_recv().is_err());
                assert_eq!(a.draft(), "safe\n");
            }
        }
    }
}
#[test]
fn shift_enter_always_newline_and_only_confirmed_encoded_unmodified_enter_submits() {
    let (tx, rx) = mpsc::channel();
    for confirmed in [false, true] {
        let mut a = App::demo();
        a.keyboard_confirmed = confirmed;
        a.append("中文");
        key(&mut a, encoded(KeyModifiers::SHIFT), &tx);
        assert_eq!(a.draft(), "中文\n");
        assert!(rx.try_recv().is_err());
    }
    let mut a = App::demo();
    a.append("explicit");
    key(&mut a, encoded(KeyModifiers::NONE), &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
}
#[test]
fn delayed_legacy_event_does_not_depend_on_current_modifier_snapshot() {
    // Same raw event before/after unrelated later input; no OS snapshot can
    // change its routing or promote it into an encoded send event.
    let (tx, rx) = mpsc::channel();
    let mut a = App::demo();
    a.append("queued");
    let stale = event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    a.input(event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    key(&mut a, stale, &tx);
    assert_eq!(a.draft(), "queuedx\n");
    assert!(rx.try_recv().is_err());
}
#[test]
fn encoded_enter_is_command_boundary_and_paste_focus_repeat_guards_still_hold() {
    use upstream_input::TimedInputEvent as T;
    let normalized = upstream_input::normalize_batch(vec![
        T::now(Event::Key(event::KeyEvent::from(KeyCode::Char('a')))),
        T::now(Event::Key(encoded(KeyModifiers::NONE))),
        T::now(Event::Key(event::KeyEvent::from(KeyCode::Char('b')))),
    ]);
    assert!(normalized.iter().any(
        |e| matches!(e.event,Event::Key(k) if k.state.contains(event::KeyEventState::CSI_U_ENTER))
    ));
    let (tx, rx) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    let mut a = App::demo();
    dispatch_input(&mut a, Event::Paste("literal\n".into()), area, area, &tx);
    key(&mut a, encoded(KeyModifiers::NONE), &tx);
    assert!(rx.try_recv().is_err());
    a.paste_in_batch = false;
    a.last_paste = None;
    a.focus_in_batch = true;
    key(&mut a, encoded(KeyModifiers::NONE), &tx);
    assert!(rx.try_recv().is_err());
    a.focus_in_batch = false;
    let mut repeat = encoded(KeyModifiers::NONE);
    repeat.kind = KeyEventKind::Repeat;
    key(&mut a, repeat, &tx);
    assert!(rx.try_recv().is_err());
}
#[test]
fn capability_footer_truthfully_advertises_shift_or_disabled_send() {
    for confirmed in [false, true] {
        let mut a = App::demo();
        a.keyboard_confirmed = confirmed;
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        t.draw(|f| draw(f, &a)).unwrap();
        let text: String = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        let text = text.replace(' ', "");
        assert!(!text.contains("Alt+Enter"));
        assert_eq!(text.contains("Shift+Enter:换行"), confirmed);
        assert_eq!(text.contains("发送已禁用"), !confirmed);
    }
}
