//! Integration checks for the original Grok prompt viewport and mouse routing.
use super::*;
use ratatui::backend::TestBackend;
fn paint(a: &App, area: Rect) {
    ui::settle(a, area);
    let mut t = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    t.draw(|f| ui::draw(f, a)).unwrap();
}
fn wheel(a: &mut App, area: Rect, kind: MouseEventKind, x: u16, y: u16) {
    let (tx, _) = mpsc::channel();
    mouse(
        a,
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &tx,
    );
}
#[test]
fn prompt_uses_original_half_terminal_cap_and_footer_does_not_move() {
    for h in [18, 24, 30, 60] {
        let area = Rect::new(0, 0, 100, h);
        let mut a = App::demo();
        paint(&a, area);
        assert_eq!(ui::prompt_hit_area(&a, area).height, 3);
        a.append(&"line\n".repeat(100));
        paint(&a, area);
        let r = ui::prompt_hit_area(&a, area);
        assert_eq!(r.height, h / 2);
        assert!(r.bottom() < h);
    }
}
#[test]
fn wheel_in_prompt_uses_original_scroll_override_without_moving_history_or_caret() {
    let area = Rect::new(0, 0, 100, 30);
    let mut a = App::demo();
    a.append(&"line\n".repeat(60));
    paint(&a, area);
    let r = a.composer_area.get();
    let caret = a.draft.cursor();
    let history = a.scroll;
    assert_eq!(a.draft.scroll_override(), None);
    wheel(&mut a, area, MouseEventKind::ScrollUp, r.x + 1, r.y + 1);
    let offset = a
        .draft
        .scroll_override()
        .expect("vendored textarea accepted wheel");
    assert_eq!(a.scroll, history);
    assert_eq!(a.draft.cursor(), caret);
    paint(&a, area);
    wheel(&mut a, area, MouseEventKind::ScrollUp, r.x + 1, r.y + 1);
    assert!(a.draft.scroll_override().unwrap() < offset);
    assert_eq!(a.scroll, history);
    a.append("x");
    assert_eq!(
        a.draft.scroll_override(),
        None,
        "editing resumes caret following"
    );
}
