use super::*;
use ratatui::backend::{Backend, TestBackend};

#[test]
fn placeholder_and_draft_styles_reach_the_rendered_buffer() {
    let mut app = App::demo();
    let area = Rect::new(0, 0, 100, 30);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let prompt = layout(&app, area).prompt;
    let (x, y) = (prompt.x + 4, prompt.y + 1);
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    let cell = &terminal.backend().buffer()[(x, y)];
    assert_eq!(cell.symbol(), " ", "focused Grok prompt has no placeholder");
    assert_eq!(cell.fg, Color::Rgb(225, 225, 225));
    assert_eq!(
        terminal.backend().buffer()[(prompt.x, prompt.y)].fg,
        Color::Rgb(80, 80, 88)
    );
    assert_eq!(terminal.backend().buffer()[(prompt.x + 2, y)].symbol(), "❯");
    assert_eq!(
        terminal.backend().buffer()[(prompt.x + 2, y)].fg,
        Color::Rgb(200, 200, 200)
    );
    assert_eq!(cell.bg, Color::Rgb(20, 20, 20));
    terminal.backend_mut().assert_cursor_position((x, y));
    app.append("中文");
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "中");
    assert_eq!(
        terminal.backend().buffer()[(x, y)].fg,
        Color::Rgb(225, 225, 225)
    );
    terminal.backend_mut().assert_cursor_position((x + 4, y));
    app.edit(crossterm::event::KeyCode::Left);
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((x + 2, y));
}

#[test]
fn row_style_is_inherited_but_explicit_span_style_wins() {
    let row = Line::styled("hint", Style::default().fg(Color::Red));
    let lines = upstream::user_lines(vec![row], "❯ ");
    assert!(
        lines[0]
            .spans
            .iter()
            .all(|s| s.style.fg == Some(Color::Red))
    );
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
    let (x, y) = (prompt.x + 4, prompt.y + 1);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    app.append("e\u{301}👩‍💻");
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(x + 1, y)].symbol(), "👩‍💻");
    terminal.backend_mut().assert_cursor_position((x + 3, y));
    app.edit(crossterm::event::KeyCode::Left);
    app.edit(crossterm::event::KeyCode::Backspace);
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "👩‍💻");
    terminal.backend_mut().assert_cursor_position((x, y));
    app.edit(crossterm::event::KeyCode::Delete);
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), " ");
    assert_eq!(
        terminal.backend().buffer()[(x, y)].fg,
        Color::Rgb(225, 225, 225)
    );
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
            app.edit(code);
            let area = layout(&app, Rect::new(0, 0, w, h)).prompt;
            settle(&app, Rect::new(0, 0, w, h));
            terminal.draw(|f| draw(f, &app)).unwrap();
            let caret = terminal.backend_mut().get_cursor_position().unwrap();
            assert!(caret.x >= area.x + 4 && caret.x < area.right() - 1);
            assert!(caret.y > area.y && caret.y < area.bottom() - 1);
            let ta_area = Rect::new(
                area.x + 4,
                area.y + 1,
                upstream::content_width(area.width).saturating_sub(2),
                area.height.saturating_sub(2),
            );
            assert_eq!(
                Some((caret.x, caret.y)),
                app.draft
                    .cursor_pos_with_state(ta_area, *app.draft_state.borrow())
            );
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
    app.expanded_messages.insert("1".into(), true); // Scroll fixture must exceed viewport, independently of prompt collapse.
    settle(&app, area);
    let timeline = timeline(&app, area);
    let track = timeline.track.unwrap();
    assert_eq!(track.x, timeline.content.right() + 1);
    assert_eq!(track.width, 1);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    app.follow_latest = false;
    app.scroll = 0;
    settle(&app, Rect::new(0, 0, 100, 30));
    terminal.draw(|f| draw(f, &app)).unwrap();
    let top = terminal.backend().buffer()[(track.x, track.y)].clone();
    assert_eq!(top.symbol(), "█");
    assert_eq!(
        top.bg, top.fg,
        "full-cell fill prevents Terminal line-gap stripes"
    );
    app.follow_latest = true;
    app.scroll = timeline.limit();
    settle(&app, Rect::new(0, 0, 100, 30));
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
    settle(&app, Rect::new(0, 0, 100, 30));
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

#[test]
fn prompt_matches_original_textarea_at_same_character_grids() {
    use ratatui::widgets::StatefulWidgetRef;
    for (w, h) in [(100, 30), (120, 40)] {
        for text in ["", "中文 e\u{301}\nsecond\nthird"] {
            let mut app = App::demo();
            app.append(text);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let prompt = layout(&app, Rect::new(0, 0, w, h)).prompt;
            assert_eq!(prompt.x, 2);
            let ta = Rect::new(6, prompt.y + 1, w - 9, prompt.height - 2);
            let mut reference = Buffer::empty(Rect::new(0, 0, w, h));
            reference.set_style(
                ta,
                Style::default()
                    .fg(Color::Rgb(225, 225, 225))
                    .bg(Color::Rgb(20, 20, 20)),
            );
            let mut original = xai_ratatui_textarea::TextArea::new();
            original.set_text(text);
            original.set_cursor(text.len());
            let mut state = xai_ratatui_textarea::TextAreaState::default();
            (&original).render_ref(ta, &mut reference, &mut state);
            for y in ta.top()..ta.bottom() {
                for x in ta.left()..ta.right() {
                    assert_eq!(
                        terminal.backend().buffer()[(x, y)],
                        reference[(x, y)],
                        "{w}x{h} at {x},{y}"
                    );
                }
            }
            terminal
                .backend_mut()
                .assert_cursor_position(original.cursor_pos_with_state(ta, state).unwrap());
        }
    }
}

#[test]
fn footer_complete_hints_and_hits_share_full_width_offset_geometry() {
    let hints = footer_labels(&App::demo());
    let total: u16 =
        hints.iter().map(|s| s.width() as u16).sum::<u16>() + 5 * (hints.len() as u16 - 1);
    for width in [42, total + 3, total + 4, total + 5, 120] {
        for (x, y) in [(0, 0), (7, 5)] {
            let area = Rect::new(x, y, width, 30);
            let mut t = Terminal::new(TestBackend::new(area.right(), area.bottom())).unwrap();
            t.draw(|f| footer(f, &App::demo(), area, true)).unwrap();
            let geometry = footer_geometry(area, &hints);
            let row = area.bottom() - 2;
            assert_eq!(geometry.len() == hints.len(), width >= total + 4);
            assert_eq!(
                (area.x..area.right())
                    .filter(|x| t.backend().buffer()[(*x, row)].symbol() == "│")
                    .count(),
                geometry.len() - 1
            );
            for (_i, (r, sep)) in geometry.iter().enumerate() {
                assert!(r.right() <= area.right() - 2);
                for col in r.x..r.right() {
                    assert_eq!(
                        hit_test(area, col, row),
                        None // Footer now contains shortcuts, not removed Earlier/Refresh actions.
                    );
                }
                if let Some(sep) = sep {
                    for col in sep.x..sep.right() {
                        assert_eq!(hit_test(area, col, row), None);
                    }
                }
            }
            let text: String = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(!text.contains("审阅发送"));
            assert_eq!(hit_test(area, area.right() - 4, row), None);
        }
    }
}

#[test]
fn footer_wide_row_matches_upstream_shortcut_span_loop_at_same_grid() {
    let hints = footer_labels(&App::demo());
    // Source: shortcuts_bar.rs::render at fixed 2bdd1d6. Same ordered labels
    // as the dot adapter; reproduce upstream's set_span/Unicode-width loop.
    // Whole-item preflight only changes the narrow truncation case (above).
    for (x, y, width) in [(0, 0, 100), (7, 5, 120)] {
        let area = Rect::new(x, y, width, 30);
        let mut terminal = Terminal::new(TestBackend::new(x + width, y + 30)).unwrap();
        let app = App::demo();
        terminal.draw(|f| footer(f, &app, area, true)).unwrap();
        let content = upstream::compute(area, 1).footer;
        let mut expected = Buffer::empty(content);
        let mut col = content.x;
        let style = Style::default();
        for (i, text) in hints.iter().enumerate() {
            if i > 0 {
                let sep = Span::styled("  │  ", style);
                let sep_width = 5u16;
                if col + sep_width > content.right() {
                    break;
                }
                expected.set_span(col, content.y, &sep, sep_width);
                col += sep_width;
            }
            let (key, label) = text.split_once(':').unwrap_or(("", text));
            let key_width = key.width() as u16;
            if col + key_width > content.right() {
                break;
            }
            expected.set_span(col, content.y, &Span::styled(key, style), key_width);
            col += key_width;
            if !key.is_empty() {
                expected.set_span(col, content.y, &Span::styled(":", style), 1);
                col += 1;
            }
            let action_width = label.width() as u16;
            if col + action_width > content.right() {
                break;
            }
            expected.set_span(col, content.y, &Span::styled(label, style), action_width);
            col += action_width;
        }
        for c in content.x..col {
            assert_eq!(
                terminal.backend().buffer()[(c, content.y)].symbol(),
                expected[(c, content.y)].symbol()
            );
        }
        assert_eq!(content.x, area.x + 2);
        assert_eq!(content.right(), area.right() - 2);
    }
}

#[test]
fn footer_keys_colon_actions_and_separators_use_actual_groknight_styles() {
    let hints = footer_labels(&App::demo());
    for area in [Rect::new(0, 0, 100, 30), Rect::new(7, 5, 60, 30)] {
        let mut terminal = Terminal::new(TestBackend::new(area.right(), area.bottom())).unwrap();
        let app = App::demo();
        // Repaint over bold/dim foreign style, testing explicit modifier reset.
        terminal
            .draw(|f| {
                f.buffer_mut().set_style(
                    area,
                    Style::default().add_modifier(Modifier::BOLD | Modifier::DIM),
                );
                footer(f, &app, area, true);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        for (i, (rect, sep)) in footer_geometry(area, &hints).iter().enumerate() {
            let key_width = hints[i]
                .split_once(':')
                .map_or(0, |(key, _)| key.width() as u16);
            let mut x = rect.x;
            while x < rect.right() {
                let cell = &buf[(x, rect.y)];
                assert_eq!(cell.bg, Color::Rgb(20, 20, 20));
                assert_eq!(
                    cell.fg,
                    if x < rect.x + key_width {
                        Color::Rgb(200, 200, 200)
                    } else {
                        Color::Rgb(108, 108, 108)
                    }
                );
                assert_eq!(
                    cell.modifier.contains(Modifier::BOLD),
                    x < rect.x + key_width
                );
                assert!(!cell.modifier.contains(Modifier::DIM));
                // Ratatui resets the hidden continuation cell of a wide glyph.
                x += cell.symbol().width().max(1) as u16;
            }
            if let Some(sep) = sep {
                let cell = &buf[(sep.x + 2, sep.y)];
                assert_eq!(cell.fg, Color::Rgb(108, 108, 108));
                assert_eq!(cell.bg, Color::Rgb(20, 20, 20));
                assert!(cell.modifier.contains(Modifier::DIM));
                assert!(!cell.modifier.contains(Modifier::BOLD));
            }
        }
    }
}

#[test]
fn fenced_code_keeps_original_renderer_spans_in_history() {
    for width in [42usize, 80, 120] {
        let text = "```rust\nlet answer = 42;\n\nprintln!(\"你好\");\n```";
        let m = crate::backend::Message {
            id: "code".into(),
            role: "dot".into(),
            text: text.into(),
            created_at: String::new(),
            files: vec![],
        };
        let (rows, _) = transcript_layout(&[m], width, &Default::default());
        let original = assistant_lines(
            text,
            width.saturating_sub(4 + upstream::timestamp_reserved(false) as usize),
        );
        // The history adapter may add gutters, but must preserve every original
        // highlighted span, blank row and code background unchanged.
        for (row, source) in rows.iter().zip(original.iter()) {
            assert_eq!(
                &row.spans[1..1 + source.spans.len()],
                source.spans.as_slice()
            );
            if source.style.bg.is_some() {
                assert_eq!(row.style.bg, source.style.bg);
            }
        }
        assert!(
            !rows
                .iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.content.contains("```"))
        );
    }
}
