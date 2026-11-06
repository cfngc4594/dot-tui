use super::*;
use std::{cell::RefCell, rc::Rc};
#[derive(Debug, Default)]
struct ClipState {
    text: Option<String>,
    reads: usize,
    writes: Vec<String>,
}
#[derive(Debug)]
struct FakeClip(Rc<RefCell<ClipState>>);
impl xai_ratatui_textarea::ClipboardProvider for FakeClip {
    fn get(&mut self) -> Option<String> {
        let mut s = self.0.borrow_mut();
        s.reads += 1;
        s.text.clone()
    }
    fn set(&mut self, text: &str) {
        let mut s = self.0.borrow_mut();
        s.text = Some(text.into());
        s.writes.push(text.into());
    }
}
fn fixture() -> (App, Rc<RefCell<ClipState>>) {
    let a = App::demo();
    let state = Rc::new(RefCell::new(ClipState::default()));
    a.clipboard.replace(Box::new(FakeClip(state.clone())));
    (a, state)
}
fn k(code: KeyCode, mods: KeyModifiers) -> event::KeyEvent {
    event::KeyEvent::new(code, mods)
}
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
            text: format!("MSG-{i:03}\n{}", "history rows\n".repeat(4)),
            files: vec![],
            created_at: "2026-11-04T06:29:00Z".into(),
        })
        .collect()
}
fn settle(a: &mut App, area: Rect) {
    ui::settle(a, area);
    sync_history_position(a, area);
}
#[test]
fn focus_lost_blocks_keys_paste_copy_and_hides_caret() {
    let (mut a, clip) = fixture();
    a.append("keep");
    a.draft.set_selection(0, 4);
    let (tx, rx) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    assert_eq!(
        dispatch_input(&mut a, Event::FocusLost, area, area, &tx),
        InputFlow::Paint
    );
    assert!(!key(
        &mut a,
        k(KeyCode::Char('q'), KeyModifiers::CONTROL),
        &tx
    ));
    key(&mut a, k(KeyCode::Char('c'), KeyModifiers::CONTROL), &tx);
    dispatch_input(&mut a, Event::Paste("bad".into()), area, area, &tx);
    key(&mut a, enter(), &tx);
    assert_eq!(a.draft(), "keep");
    assert!(clip.borrow().writes.is_empty());
    assert!(rx.try_recv().is_err());
    assert!(ui::cursor_position(&a, area).is_none());
}
#[test]
fn gained_focus_restores_idle_prompt_but_not_modal_busy_or_pending() {
    let area = Rect::new(0, 0, 100, 30);
    let (tx, rx) = mpsc::channel();
    for state in 0..4 {
        let mut a = App::demo();
        a.prompt_focused = false;
        a.window_focused = false;
        a.follow_latest = false;
        match state {
            1 => a.busy = true,
            2 => a.quit_review = true,
            3 => a.uncertain = true,
            _ => (),
        };
        dispatch_input(&mut a, Event::FocusGained, area, area, &tx);
        assert!(a.window_focused && a.reassert_mouse_capture && a.focus_in_batch);
        assert_eq!(a.prompt_focused, state == 0);
        assert!(!a.follow_latest);
        a.append("draft");
        key(&mut a, enter(), &tx);
        assert!(rx.try_recv().is_err());
    }
}
#[test]
fn focus_loss_cancels_mouse_latches_and_queued_focus_gain_cannot_submit() {
    let mut a = App::demo();
    a.append("queued");
    let (tx, rx) = mpsc::channel();
    let area = Rect::new(0, 0, 100, 30);
    a.scrollbar_drag = Some(area);
    a.history_press = Some(("1".into(), 3, 5));
    a.prompt_mouse_down = true;
    dispatch_input(&mut a, Event::FocusLost, area, area, &tx);
    assert!(a.scrollbar_drag.is_none() && a.history_press.is_none() && !a.prompt_mouse_down);
    dispatch_input(&mut a, Event::FocusGained, area, area, &tx);
    dispatch_input(&mut a, Event::Key(enter()), area, area, &tx);
    assert!(rx.try_recv().is_err());
}
#[test]
fn page_keys_are_owned_by_pane_and_modifiers_stay_with_textarea() {
    let mut a = App::demo();
    a.messages = history(0, 64);
    let area = Rect::new(0, 0, 100, 30);
    settle(&mut a, area);
    a.follow_latest = false;
    a.scroll = 25;
    let (tx, rx) = mpsc::channel();
    for code in [KeyCode::PageUp, KeyCode::PageDown] {
        key(&mut a, k(code, KeyModifiers::NONE), &tx);
        assert_eq!(a.scroll, 25);
    }
    a.prompt_focused = false;
    key(&mut a, k(KeyCode::PageUp, KeyModifiers::SHIFT), &tx);
    assert_eq!(a.scroll, 25);
    key(&mut a, k(KeyCode::PageUp, KeyModifiers::NONE), &tx);
    assert!(a.scroll < 25);
    let up = a.scroll;
    key(&mut a, k(KeyCode::PageDown, KeyModifiers::NONE), &tx);
    assert!(a.scroll > up);
    assert!(rx.try_recv().is_err());
}
#[test]
fn ctrl_c_and_cmd_c_copy_selection_without_exit_or_draft_change() {
    let (mut a, clip) = fixture();
    a.append("A中e\u{301}Z");
    a.draft.set_selection(1, "A中e\u{301}".len());
    let (tx, rx) = mpsc::channel();
    for mods in [KeyModifiers::CONTROL, KeyModifiers::SUPER] {
        assert!(!key(&mut a, k(KeyCode::Char('c'), mods), &tx));
    }
    assert_eq!(clip.borrow().writes, vec!["中e\u{301}", "中e\u{301}"]);
    assert_eq!(a.draft(), "A中e\u{301}Z");
    assert_eq!(clip.borrow().reads, 0);
    assert!(rx.try_recv().is_err());
    a.draft.clear_selection();
    assert!(!key(
        &mut a,
        k(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &tx
    ));
    assert!(key(
        &mut a,
        k(KeyCode::Char('q'), KeyModifiers::CONTROL),
        &tx
    ));
}
#[test]
fn shared_provider_survives_clear_and_accepted_send_and_pastes_once_safely() {
    let (mut a, clip) = fixture();
    let (tx, rx) = mpsc::channel();
    a.append("copy 中文");
    let end = a.draft().len();
    a.draft.set_selection(0, end);
    key(&mut a, k(KeyCode::Char('c'), KeyModifiers::CONTROL), &tx);
    key(&mut a, k(KeyCode::Char('u'), KeyModifiers::CONTROL), &tx);
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::CONTROL), &tx);
    assert_eq!(a.draft(), "copy 中文");
    assert_eq!(clip.borrow().reads, 1);
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    a.paste_in_batch = false;
    a.last_paste = None;
    key(&mut a, enter(), &tx);
    let Job::Send(g, p) = rx.try_recv().unwrap() else {
        panic!()
    };
    a.apply(Reply::Sent(g, p.key, Ok("accepted".into())), &tx);
    rx.try_recv().unwrap();
    a.busy = false;
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::SUPER), &tx);
    assert_eq!(a.draft(), "copy 中文");
    assert_eq!(clip.borrow().reads, 2);
}
#[test]
fn paste_has_one_provider_read_and_uses_sanitizer_size_and_window_guards() {
    let (mut a, clip) = fixture();
    clip.borrow_mut().text = Some(format!("\x1b[31m{}\n", "中".repeat(17000)));
    let (tx, rx) = mpsc::channel();
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::CONTROL), &tx);
    assert_eq!(clip.borrow().reads, 1);
    assert!(!a.draft().contains('\x1b'));
    assert_eq!(a.draft().chars().count(), 16000);
    key(&mut a, enter(), &tx);
    assert!(rx.try_recv().is_err());
    a.window_focused = false;
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::SUPER), &tx);
    a.window_focused = true;
    a.prompt_focused = false;
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::CONTROL), &tx);
    a.prompt_focused = true;
    a.uncertain = true;
    key(&mut a, k(KeyCode::Char('v'), KeyModifiers::CONTROL), &tx);
    assert_eq!(clip.borrow().reads, 1);
}
#[test]
fn latest_poll_runs_while_reading_old_history_and_restores_anchor_after_merge() {
    let mut a = App::demo();
    a.messages = history(0, 64);
    a.follow_latest = false;
    a.prompt_focused = false;
    let area = Rect::new(0, 0, 100, 30);
    settle(&mut a, area);
    a.scroll = 7;
    let anchor = ui::reading_anchor(&a).unwrap();
    let (tx, rx) = mpsc::channel();
    poll_latest(&mut a, &tx);
    assert!(matches!(rx.try_recv(), Ok(Job::Page(_, _, None))));
    assert!(!a.follow_latest);
    a.apply(Reply::Page(0, false, Ok(history(40, 32))), &tx);
    assert_eq!(a.messages.len(), 72);
    settle(&mut a, area);
    let after = ui::reading_anchor(&a).unwrap();
    assert_eq!((anchor.id, anchor.row), (after.id, after.row));
    assert!(!a.follow_latest);
    assert!(a.last_result.contains("新增 8"));
    a.apply(Reply::Page(0, false, Ok(history(40, 32))), &tx);
    settle(&mut a, area);
    assert_eq!(a.messages.len(), 72);
}
#[test]
fn newest_poll_does_not_erase_history_or_older_boundary_on_empty_page() {
    let mut a = App::demo();
    a.messages = history(0, 64);
    a.older_exhausted = false;
    a.follow_latest = false;
    let (tx, _) = mpsc::channel();
    let before = a.messages.clone();
    a.apply(Reply::Page(0, false, Ok(Vec::new())), &tx);
    assert_eq!(a.messages, before);
    assert!(!a.older_exhausted);
}
#[test]
fn bounded_merge_keeps_old_anchor_and_new_tail_and_reports_omission() {
    let mut a = App::demo();
    a.messages = history(0, 288);
    a.follow_latest = false;
    let area = Rect::new(0, 0, 100, 30);
    settle(&mut a, area);
    a.scroll = 0;
    let anchor = ui::reading_anchor(&a).unwrap();
    let (tx, _) = mpsc::channel();
    a.apply(Reply::Page(0, false, Ok(history(280, 32))), &tx);
    settle(&mut a, area);
    assert_eq!(a.messages.len(), 288);
    assert_eq!(ui::reading_anchor(&a).unwrap().id, anchor.id);
    assert!(a.messages.iter().any(|m| m.id == "000311"));
    assert!(a.last_result.contains("省略"));
}
#[test]
fn background_updates_do_not_change_window_focus_draft_or_uncertainty() {
    let (mut a, clip) = fixture();
    a.messages = history(0, 32);
    a.append("draft");
    a.window_focused = false;
    a.prompt_focused = false;
    a.follow_latest = false;
    a.uncertain = true;
    let (tx, _) = mpsc::channel();
    a.apply(Reply::Page(0, false, Ok(history(24, 32))), &tx);
    assert_eq!(a.messages.len(), 56);
    assert_eq!(a.draft(), "draft");
    assert!(!a.window_focused && !a.prompt_focused && !a.follow_latest && a.uncertain);
    assert_eq!(clip.borrow().reads, 0);
}
#[test]
fn latest_error_preserves_page_and_pauses_automatic_requests() {
    let mut a = App::demo();
    a.messages = history(0, 64);
    a.follow_latest = false;
    let (tx, rx) = mpsc::channel();
    let before = a.messages.clone();
    a.apply(Reply::Page(0, false, Err("offline".into())), &tx);
    poll_latest(&mut a, &tx);
    assert!(rx.try_recv().is_err());
    assert_eq!(a.messages, before);
    assert!(a.poll_paused);
}

#[test]
fn latest_merge_can_anchor_last_visible_frame_while_new_geometry_is_pending() {
    let mut a = App::demo();
    a.messages = history(0, 64);
    a.follow_latest = false;
    let area = Rect::new(0, 0, 100, 30);
    settle(&mut a, area);
    a.scroll = 8;
    let before = ui::reading_anchor(&a).unwrap();
    // Growing the composer changes body height before the layout worker replies.
    a.append("one\ntwo\nthree\nfour");
    let _ = ui::timeline(&a, area);
    let (tx, _) = mpsc::channel();
    a.apply(Reply::Page(0, false, Ok(history(40, 32))), &tx);
    assert!(a.reading_anchor.is_some());
    settle(&mut a, area);
    let after = ui::reading_anchor(&a).unwrap();
    assert_eq!((before.id, before.row), (after.id, after.row));
}
