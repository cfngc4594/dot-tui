use super::*;
use ratatui::backend::{Backend, TestBackend};

#[test]
fn placeholder_and_draft_styles_reach_the_rendered_buffer() {
    let mut app = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let prompt = layout(&app, area).prompt;
    let (x, y) = (prompt.x + 5, prompt.y + 1);
    terminal.draw(|f| draw(f, &app)).unwrap();
    let cell = &terminal.backend().buffer()[(x, y)];
    assert_eq!(cell.symbol(), "给");
    assert_eq!(cell.fg, upstream::MUTED);
    terminal.backend_mut().assert_cursor_position((x, y));
    app.append("中文");
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "中");
    assert_eq!(terminal.backend().buffer()[(x, y)].fg, upstream::TEXT);
    terminal.backend_mut().assert_cursor_position((x + 4, y));
    app.edit(crossterm::event::KeyCode::Left);
    terminal.draw(|f| draw(f, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((x + 2, y));
}

#[test]
fn row_style_is_inherited_but_explicit_span_style_wins() {
    let row = Line::styled("hint", Style::default().fg(Color::Red));
    let lines = upstream::user_lines(vec![row], "❯ ");
    assert!(lines[0]
        .spans
        .iter()
        .all(|s| s.style.fg == Some(Color::Red)));
    let row = Line::from(Span::styled("override", Style::default().fg(Color::Green)))
        .style(Style::default().fg(Color::Red));
    let lines = upstream::user_lines(vec![row], "❯ ");
    assert_eq!(lines[0].spans[1].style.fg, Some(Color::Green));
}

#[test]
fn combining_and_zwj_graphemes_have_real_buffer_cells_and_caret_widths() {
    let mut app = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    let prompt = layout(&app, area).prompt;
    let (x, y) = (prompt.x + 5, prompt.y + 1);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    app.append("e\u{301}👩‍💻");
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(x + 1, y)].symbol(), "👩‍💻");
    terminal.backend_mut().assert_cursor_position((x + 3, y));
    app.edit(crossterm::event::KeyCode::Left);
    app.edit(crossterm::event::KeyCode::Backspace);
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "👩‍💻");
    terminal.backend_mut().assert_cursor_position((x, y));
    app.edit(crossterm::event::KeyCode::Delete);
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "给");
    assert_eq!(terminal.backend().buffer()[(x, y)].fg, upstream::MUTED);
}

#[test]
fn cursor_and_visible_draft_share_wraps_offsets_and_resize_geometry() {
    let mut app = App::demo();
    app.append(&"中文 e\u{301} 👩‍💻\n".repeat(30));
    for (w, h) in [(42, 18), (100, 30), (60, 22)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        for code in [
            crossterm::event::KeyCode::End,
            crossterm::event::KeyCode::Home,
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyCode::Left,
        ] {
            app.editor_width = input_width(&app, Rect::new(0, 0, w, h));
            app.edit(code);
            let area = layout(&app, Rect::new(0, 0, w, h)).prompt;
            terminal.draw(|f| draw(f, &app)).unwrap();
            let caret = terminal.backend_mut().get_cursor_position().unwrap();
            assert!(caret.x >= area.x + 5 && caret.x < area.right() - 1);
            assert!(caret.y > area.y && caret.y < area.bottom() - 1);
            let view = super::super::editor::layout(
                app.draft(),
                app.draft_cursor(),
                input_width(&app, Rect::new(0, 0, w, h)),
                area.height as usize - 2,
            );
            assert_eq!(caret.x, area.x + 5 + view.cursor.1 as u16);
            assert_eq!(caret.y, area.y + 1 + (view.cursor.0 - view.offset) as u16);
        }
    }
}

#[test]
fn scrollbar_is_conditional_and_buffer_thumb_moves_with_position_and_follow_mode() {
    let area = Rect::new(0, 0, 100, 30);
    let mut app = App::demo();
    app.messages.clear();
    assert!(timeline(&app, area).track.is_none());
    app.messages = App::demo().messages;
    app.messages[0].text = "row\n".repeat(100);
    let timeline = timeline(&app, area);
    let track = timeline.track.unwrap();
    assert_eq!(track.x, timeline.content.right() + 1);
    assert_eq!(track.width, 1);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    app.follow_latest = false;
    app.scroll = 0;
    terminal.draw(|f| draw(f, &app)).unwrap();
    let top = terminal.backend().buffer()[(track.x, track.y)].clone();
    assert_eq!(top.symbol(), "█");
    assert_eq!(
        top.bg, top.fg,
        "full-cell fill prevents Terminal line-gap stripes"
    );
    app.follow_latest = true;
    app.scroll = timeline.limit();
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_ne!(
        terminal.backend().buffer()[(track.x, track.y)].symbol(),
        "█"
    );
    let bottom = &terminal.backend().buffer()[(track.x, track.bottom() - 1)];
    assert_eq!(bottom.symbol(), "█");
    assert_ne!(bottom.fg, top.fg, "follow mode is dimmer");
    assert!(
        super::super::scrollbar::scrollbar_grab_zone(track).contains((track.x - 1, track.y).into())
    );
    assert!(timeline.content.height > 0);
    app.messages.clear();
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_ne!(
        terminal.backend().buffer()[(track.x, track.y)].symbol(),
        "█",
        "rail is removed when content shrinks"
    );
}

#[test]
fn exact_fit_uses_full_width_and_narrow_view_has_no_rail() {
    let mut app = App::demo();
    app.messages.clear();
    let area = Rect::new(0, 0, 100, 30);
    assert_eq!(timeline(&app, area).content, layout(&app, area).body);
    assert!(timeline(&app, Rect::new(0, 0, 41, 18)).track.is_none());
    assert!(!super::super::scrollbar::needs_scrollbar(10, 10));
    assert!(super::super::scrollbar::needs_scrollbar(11, 10));
}
