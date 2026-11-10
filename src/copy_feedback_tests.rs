//! End-to-end mouse/keyboard copies with an injected clipboard, never the system pasteboard.
use super::*;
use crate::scrollback::text_selection::ResolvedSelectableLine;
use ratatui::{backend::TestBackend, buffer::Buffer};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Default)]
struct Copies {
    reads: usize,
    writes: Vec<String>,
}
#[derive(Debug)]
struct RecordingClipboard(Rc<RefCell<Copies>>);
impl xai_ratatui_textarea::ClipboardProvider for RecordingClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.borrow_mut().reads += 1;
        self.0.borrow().writes.last().cloned()
    }
    fn set(&mut self, text: &str) {
        self.0.borrow_mut().writes.push(text.into());
    }
}
fn fixture(text: &str, area: Rect) -> (App, Rc<RefCell<Copies>>) {
    let mut app = App::demo();
    app.messages = vec![Message {
        id: "copy-feedback-fixture".into(),
        role: "dot".into(),
        text: text.into(),
        files: vec![],
        created_at: "2026-11-04T06:29:00Z".into(),
    }];
    let copies = Rc::new(RefCell::new(Copies::default()));
    app.clipboard
        .replace(Box::new(RecordingClipboard(copies.clone())));
    ui::settle(&app, area);
    (app, copies)
}
fn rows(app: &App, area: Rect) -> Vec<ResolvedSelectableLine> {
    ui::selection_model(app, area)
        .0
        .ranges
        .into_iter()
        .flat_map(|r| r.lines)
        .collect()
}
fn start(row: &ResolvedSelectableLine) -> (u16, u16) {
    (row.screen_x + row.selectable_cols.start, row.screen_y)
}
fn paint(app: &App, area: Rect) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}
fn find(buf: &Buffer, text: &str) -> Option<(u16, u16)> {
    (0..buf.area.height).find_map(|y| {
        let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        line.find(text)
            .map(|byte| (line[..byte].chars().count() as u16, y))
    })
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
fn click(app: &mut App, area: Rect, at: (u16, u16), tx: &mpsc::Sender<Job>) {
    pointer(app, area, MouseEventKind::Down(MouseButton::Left), at, tx);
    pointer(app, area, MouseEventKind::Up(MouseButton::Left), at, tx);
}
fn drag(app: &mut App, area: Rect, from: (u16, u16), to: (u16, u16), tx: &mpsc::Sender<Job>) {
    pointer(app, area, MouseEventKind::Down(MouseButton::Left), from, tx);
    pointer(app, area, MouseEventKind::Drag(MouseButton::Left), to, tx);
    pointer(app, area, MouseEventKind::Up(MouseButton::Left), to, tx);
}
fn select_row(app: &mut App, area: Rect, tx: &mpsc::Sender<Job>) -> (u16, u16) {
    let row = rows(app, area).remove(0);
    let at = start(&row);
    drag(
        app,
        area,
        at,
        (row.screen_x + row.selectable_cols.end, row.screen_y),
        tx,
    );
    at
}
#[test]
fn history_copy_toast_expires_but_highlight_survives_old_flash_timeout() {
    let area = Rect::new(0, 0, 100, 30);
    let (mut app, copies) = fixture("persistent selection", area);
    let (tx, rx) = mpsc::channel();
    let original = paint(&app, area);
    let at = select_row(&mut app, area, &tx);
    let selected = paint(&app, area);
    let toast_at = find(&selected, " Copied! ").expect("release must paint a copy toast");
    let body = ui::selection_model(&app, area).0.content_area;
    assert_eq!(toast_at, (body.right() - 10, body.bottom() - 1));
    assert_ne!(selected[at].bg, original[at].bg);
    std::thread::sleep(Duration::from_millis(180));
    assert_eq!(paint(&app, area)[at].bg, selected[at].bg);
    assert_eq!(
        history_selection::wait(&app),
        None,
        "held selection must not schedule a flash timer"
    );
    app.copy_feedback.borrow_mut().show(
        copy_feedback::CopyOutcome::Copied,
        Instant::now() - Duration::from_secs(2),
    );
    let expired = paint(&app, area);
    assert!(find(&expired, "Copied!").is_none());
    assert_eq!(expired[at].bg, selected[at].bg);
    assert!(history_selection::copy_key(&mut app));
    assert_eq!(
        copies.borrow().writes,
        vec!["persistent selection", "persistent selection"]
    );
    assert_eq!(copies.borrow().reads, 0);
    assert!(rx.try_recv().is_err());
}
#[test]
fn failed_history_copy_never_renders_success() {
    let area = Rect::new(0, 0, 100, 30);
    let (mut app, _) = fixture("failure path", area);
    app.clipboard.fail_writes();
    let (tx, _) = mpsc::channel();
    select_row(&mut app, area, &tx);
    let buf = paint(&app, area);
    assert!(find(&buf, "Copy failed.").is_some());
    assert!(find(&buf, "Copied!").is_none());
}
#[test]
fn second_drag_refreshes_expired_toast_without_clipboard_reads() {
    let area = Rect::new(0, 0, 100, 30);
    let (mut app, copies) = fixture("repeat copy", area);
    let (tx, _) = mpsc::channel();
    select_row(&mut app, area, &tx);
    paint(&app, area);
    app.copy_feedback.borrow_mut().show(
        copy_feedback::CopyOutcome::Copied,
        Instant::now() - Duration::from_secs(2),
    );
    assert!(find(&paint(&app, area), "Copied!").is_none());
    select_row(&mut app, area, &tx);
    assert!(find(&paint(&app, area), "Copied!").is_some());
    assert_eq!(copies.borrow().writes, vec!["repeat copy", "repeat copy"]);
    assert_eq!(copies.borrow().reads, 0);
}
#[test]
fn prompt_mouse_release_and_cmd_c_both_show_confirmed_feedback_without_reads() {
    let area = Rect::new(0, 0, 100, 30);
    let (mut app, copies) = fixture("history", area);
    app.append("hello prompt");
    paint(&app, area);
    let prompt = ui::prompt_hit_area(&app, area);
    let at = (prompt.x + 4, prompt.y + 1);
    let (tx, rx) = mpsc::channel();
    drag(&mut app, area, at, (at.0 + 5, at.1), &tx);
    assert_eq!(copies.borrow().writes, vec!["hello"]);
    assert!(find(&paint(&app, area), "Copied!").is_some());
    app.copy_feedback.borrow_mut().show(
        copy_feedback::CopyOutcome::Copied,
        Instant::now() - Duration::from_secs(2),
    );
    dispatch_input(
        &mut app,
        Event::Key(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::SUPER,
        )),
        area,
        area,
        &tx,
    );
    assert!(find(&paint(&app, area), "Copied!").is_some());
    assert_eq!(copies.borrow().writes, vec!["hello", "hello"]);
    assert_eq!(copies.borrow().reads, 0);
    assert_eq!(app.draft(), "hello prompt");
    assert!(rx.try_recv().is_err());
}
#[test]
fn double_click_selects_word_and_url_without_folding() {
    let area = Rect::new(0, 0, 100, 30);
    for (text, offset, want) in [
        ("alpha beta gamma", 7, "beta"),
        (
            "visit https://example.com/path?q=one now",
            12,
            "https://example.com/path?q=one",
        ),
    ] {
        let (mut app, copies) = fixture(text, area);
        let row = rows(&app, area).remove(0);
        let at = (start(&row).0 + offset, row.screen_y);
        let (tx, _) = mpsc::channel();
        click(&mut app, area, at, &tx);
        click(&mut app, area, at, &tx);
        assert_eq!(
            copies.borrow().writes.last().map(String::as_str),
            Some(want)
        );
        assert!(app.expanded_messages.is_empty());
        paint(&app, area);
        assert!(history_selection::copy_key(&mut app));
        assert_eq!(
            copies.borrow().writes.last().map(String::as_str),
            Some(want)
        );
    }
}
#[test]
fn triple_click_selects_wrapped_paragraph_not_next_paragraph() {
    let area = Rect::new(0, 0, 60, 40);
    let paragraph = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
    let (mut app, copies) = fixture(&format!("{paragraph}\n\nNEXT PARAGRAPH"), area);
    let selected_rows = rows(&app, area);
    assert!(selected_rows.len() > 2, "fixture must wrap");
    let at = start(&selected_rows[1]);
    let (tx, _) = mpsc::channel();
    for _ in 0..3 {
        click(&mut app, area, at, &tx);
    }
    assert_eq!(
        copies.borrow().writes.last().map(String::as_str),
        Some(paragraph)
    );
    assert!(history_selection::copy_key(&mut app));
    assert_eq!(
        copies.borrow().writes.last().map(String::as_str),
        Some(paragraph)
    );
}
#[test]
fn escape_single_click_and_navigation_clear_persistent_selection() {
    let area = Rect::new(0, 0, 100, 30);
    for action in 0..3 {
        let (mut app, copies) = fixture("clear this selection", area);
        let (tx, _) = mpsc::channel();
        let at = select_row(&mut app, area, &tx);
        match action {
            0 => {
                dispatch_input(
                    &mut app,
                    Event::Key(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                    area,
                    area,
                    &tx,
                );
            }
            1 => click(&mut app, area, at, &tx),
            _ => {
                dispatch_input(
                    &mut app,
                    Event::Key(event::KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
                    area,
                    area,
                    &tx,
                );
            }
        }
        assert!(
            !history_selection::copy_key(&mut app),
            "action {action} retained a selection"
        );
        assert_eq!(copies.borrow().writes.len(), 1);
    }
}

#[test]
fn triple_click_table_cell_and_border_copy_cell_or_whole_grid() {
    let area = Rect::new(0, 0, 100, 35);
    let table = "| Name | Value |\n| --- | --- |\n| alpha | one |\n| beta | two |";
    for whole_table in [false, true] {
        let (mut app, copies) = fixture(table, area);
        let lines = rows(&app, area);
        let row = lines
            .iter()
            .find(|line| {
                if whole_table {
                    line.text.contains('┌')
                } else {
                    line.text.contains("alpha")
                }
            })
            .expect("rendered table row");
        let column = if whole_table {
            row.text.chars().position(|c| c == '┌').unwrap() as u16
        } else {
            row.text[..row.text.find("alpha").unwrap()].chars().count() as u16
        };
        let at = (start(row).0 + column, row.screen_y);
        let (tx, _) = mpsc::channel();
        for _ in 0..3 {
            click(&mut app, area, at, &tx);
        }
        let expected = if whole_table {
            "Name\tValue\nalpha\tone\nbeta\ttwo"
        } else {
            "alpha"
        };
        assert_eq!(
            copies.borrow().writes.last().map(String::as_str),
            Some(expected)
        );
        paint(&app, area);
        assert!(history_selection::copy_key(&mut app));
        assert_eq!(
            copies.borrow().writes.last().map(String::as_str),
            Some(expected)
        );
        assert_eq!(copies.borrow().reads, 0);
    }
}

#[test]
fn batched_double_and_triple_click_keep_the_first_toast() {
    let area = Rect::new(0, 0, 100, 30);
    for failed in [false, true] {
        let (mut app, copies) = fixture("alpha beta gamma", area);
        if failed {
            app.clipboard.fail_writes();
        }
        let row = rows(&app, area).remove(0);
        let at = (start(&row).0 + 7, row.screen_y);
        let (tx, _) = mpsc::channel();
        // The input loop can dispatch all three clicks before the next frame.
        // Triple-click feedback is debounced, but must not erase double-click feedback.
        for _ in 0..3 {
            click(&mut app, area, at, &tx);
        }
        let buffer = paint(&app, area);
        let expected = if failed { "Copy failed." } else { "Copied!" };
        assert!(
            find(&buffer, expected).is_some(),
            "lost batched {expected} toast"
        );
        if !failed {
            assert_eq!(copies.borrow().writes, vec!["beta", "alpha beta gamma"]);
        }
        assert_eq!(copies.borrow().reads, 0);
    }
}

#[test]
fn identical_latest_polls_retain_selection_but_changed_message_clears_it() {
    let area = Rect::new(0, 0, 100, 30);
    let text = "selection survives idle polling";
    let (mut app, copies) = fixture(text, area);
    let (tx, _) = mpsc::channel();
    let original = paint(&app, area);
    let at = select_row(&mut app, area, &tx);
    let held = paint(&app, area);
    assert_ne!(held[at].bg, original[at].bg);
    let snapshot = ui::selection_model(&app, area).1;
    for _ in 0..2 {
        let unchanged = app.messages.clone();
        app.apply(Reply::Page(app.generation, false, Ok(unchanged)), &tx);
        // settle() deliberately invalidates the cache for layout tests; an idle
        // production poll must leave it ready without a new layout job.
        assert!(!ui::timeline(&app, area).pending);
        assert_eq!(ui::selection_model(&app, area).1, snapshot);
        assert_eq!(paint(&app, area)[at].bg, held[at].bg);
        assert!(history_selection::copy_key(&mut app));
        assert_eq!(
            copies.borrow().writes.last().map(String::as_str),
            Some(text)
        );
        assert_eq!(history_selection::wait(&app), None);
    }
    assert_eq!(copies.borrow().writes.len(), 3);
    let mut changed = app.messages.clone();
    changed[0].text = "changed reply replaces the old selection".into();
    app.apply(Reply::Page(app.generation, false, Ok(changed)), &tx);
    ui::settle(&app, area);
    paint(&app, area);
    assert!(!history_selection::copy_key(&mut app));
    assert_eq!(
        copies.borrow().writes.len(),
        3,
        "changed reply must not recopy stale text"
    );
    assert_eq!(copies.borrow().reads, 0);
}

/// The production locale is process-wide. Run these UI assertions in a single-
/// test child so Chinese rendering cannot race other tests' English snapshots.
#[test]
fn copy_feedback_localizes_history_and_prompt_in_both_languages() {
    const CHILD: &str = "DOT_TUI_COPY_I18N_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "copy_feedback_tests::copy_feedback_localizes_history_and_prompt_in_both_languages",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "localized UI test failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }
    use i18n::Language::{Chinese, English};
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    fn assert_toast(app: &App, area: Rect, expected: &str) {
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        // Inspect the frame: Terminal's diff deliberately skips wide continuation
        // cells, so TestBackend alone does not retain their current style.
        let frame = terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buf = frame.buffer;
        let body = ui::selection_model(app, area).0.content_area;
        let padded = format!(" {expected} ");
        let mut x = body.right() - padded.width() as u16 - 1;
        let y = body.bottom() - 1;
        for grapheme in padded.graphemes(true) {
            assert_eq!(buf[(x, y)].symbol(), grapheme);
            for offset in 0..grapheme.width() as u16 {
                let cell = &buf[(x + offset, y)];
                assert_eq!(cell.bg, Color::Rgb(20, 20, 20));
                assert_eq!(cell.modifier, Modifier::BOLD);
                if offset > 0 {
                    assert_eq!(cell.symbol(), " ");
                }
            }
            x += grapheme.width() as u16;
        }
        assert_eq!(x, body.right() - 1);
    }

    let area = Rect::new(0, 0, 100, 30);
    // English and Chinese are all currently supported locales. Re-enter Chinese
    // to also catch a label cached in the locale of the first copy.
    for (language, copied, failed) in [
        (Chinese, "已复制！", "复制失败。"),
        (English, "Copied!", "Copy failed."),
        (Chinese, "已复制！", "复制失败。"),
    ] {
        i18n::set(language);
        let original = "copy 中文 e\u{301}";
        let (mut app, copies) = fixture(original, area);
        let (tx, rx) = mpsc::channel();
        let at = select_row(&mut app, area, &tx);
        assert_toast(&app, area, copied);
        assert_eq!(copies.borrow().writes, vec![original]);
        let selected_bg = paint(&app, area)[at].bg;

        // An active toast follows a language switch without another copy.
        i18n::set(if language == Chinese {
            English
        } else {
            Chinese
        });
        assert_toast(
            &app,
            area,
            if language == Chinese {
                "Copied!"
            } else {
                "已复制！"
            },
        );
        i18n::set(language);
        assert_toast(&app, area, copied);
        app.copy_feedback.borrow_mut().show(
            copy_feedback::CopyOutcome::Copied,
            Instant::now() - Duration::from_secs(2),
        );
        assert!(app.copy_feedback.borrow().message(Instant::now()).is_none());
        assert_eq!(paint(&app, area)[at].bg, selected_bg);
        app.clipboard.fail_writes();
        assert!(history_selection::copy_key(&mut app));
        assert_toast(&app, area, failed);
        assert_eq!(copies.borrow().writes, vec![original]);
        assert_eq!(copies.borrow().reads, 0);
        assert!(rx.try_recv().is_err());

        let (mut app, copies) = fixture("history", area);
        app.append("hello prompt");
        paint(&app, area);
        let prompt = ui::prompt_hit_area(&app, area);
        let at = (prompt.x + 4, prompt.y + 1);
        drag(&mut app, area, at, (at.0 + 5, at.1), &tx);
        assert_toast(&app, area, copied);
        assert_eq!(copies.borrow().writes, vec!["hello"]);
        app.clipboard.fail_writes();
        dispatch_input(
            &mut app,
            Event::Key(event::KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::SUPER,
            )),
            area,
            area,
            &tx,
        );
        assert_toast(&app, area, failed);
        assert_eq!(copies.borrow().writes, vec!["hello"]);
        assert_eq!(copies.borrow().reads, 0);
        assert_eq!(app.draft(), "hello prompt");
        assert!(rx.try_recv().is_err());
    }
}
