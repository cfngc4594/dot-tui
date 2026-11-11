//! Public, deterministic README content. Uses the same renderer, media pipeline
//! and fake transport as --demo. Initial content comes only from this module;
//! no account, network, clipboard, or private files supply the scene.
use super::{App, FakeBackend, Message};

pub const IMAGE_ID: &str = "showcase-horizon";

pub fn messages() -> Vec<Message> {
    vec![
        Message {
            id: "showcase-1".into(),
            role: "you".into(),
            text: "Give my terminal a quiet horizon, and a tiny Rust greeting.".into(),
            files: vec![],
            created_at: "2026-11-04T09:41:00Z".into(),
        },
        Message {
            id: "showcase-2".into(),
            role: "dot".into(),
            text: "**A little calm, right in your terminal.**\n\nSoft peach skies, layered mountains, and a small `hello`:\n\n```rust\nfn main() {\n    println!(\"hello, dot\");\n}\n```".into(),
            files: vec![IMAGE_ID.into()],
            created_at: "2026-11-04T09:42:00Z".into(),
        },
    ]
}

pub fn configure(app: &mut App, backend: &mut FakeBackend) {
    app.messages = messages();
    backend.messages = app.messages.clone();
    app.older_exhausted = true;
    app.append("Could you make the sky a little warmer?");
    // Hold an ordinary history-refresh state so the existing activity indicator
    // stays visible. The completed reply is not portrayed as still generating.
    // busy holds automatic polls. Sending still goes through FakeBackend only.
    app.begin("读取消息");
    // Freeze only the screenshot scene; new operations resume the real clock.
    app.showcase_elapsed_seconds = Some(1);
}

/// Original procedural artwork, Apache-2.0 like the source. No downloaded asset.
/// The wide composition reserves only ten image rows at the target viewport.
pub fn image_bytes() -> Vec<u8> {
    const W: u32 = 1200;
    const H: u32 = 200;
    let image = image::RgbImage::from_fn(W, H, |x, y| {
        let xf = x as f32 / W as f32;
        let yf = y as f32 / H as f32;
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
            image::Rgb(std::array::from_fn(|i| (a[i] * (1.0 - t) + b[i] * t) as u8))
        };
        let dx = x as f32 - 855.0;
        let dy = y as f32 - 60.0;
        let far = 0.68 - 0.22 * (xf * 13.0 + 0.5).sin() - 0.08 * (xf * 29.0).cos();
        let middle = 0.82 - 0.14 * (xf * 11.0 + 2.0).sin() - 0.06 * (xf * 23.0).cos();
        let near = 0.97 - 0.12 * (xf * 9.0 + 4.0).sin();
        if yf > near {
            mix([21.0, 54.0, 62.0], [28.0, 74.0, 77.0], xf)
        } else if yf > middle {
            mix([44.0, 81.0, 91.0], [63.0, 111.0, 116.0], xf)
        } else if yf > far {
            mix([98.0, 123.0, 134.0], [139.0, 154.0, 154.0], xf)
        } else if dx * dx + dy * dy < 26.0 * 26.0 {
            image::Rgb([255, 232, 188])
        } else {
            mix([123.0, 147.0, 171.0], [242.0, 188.0, 157.0], yf)
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "dev-demo")]
    use crate::Args;
    use crate::{Source, demo_dot, demo_image_view, native_media, ui};
    #[cfg(feature = "dev-demo")]
    use clap::Parser;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    #[test]
    #[cfg(feature = "dev-demo")]
    fn showcase_selects_fake_backend_and_rejects_auth_or_other_scenes() {
        let args = Args::try_parse_from(["dot-tui", "--showcase"]).unwrap();
        assert!(args.is_demo());
        assert!(Args::try_parse_from(["dot-tui", "--showcase", "--auth", "/secret"]).is_err());
        for flag in ["--demo-image", "--demo-incoming", "--demo-inline-fixtures"] {
            assert!(Args::try_parse_from(["dot-tui", "--showcase", "--demo", flag]).is_err());
        }
        for flag in ["--demo-history", "--demo-delay-ms"] {
            assert!(Args::try_parse_from(["dot-tui", "--showcase", "--demo", flag, "1"]).is_err());
        }
    }

    fn scene() -> App {
        let mut app = App::demo();
        let mut fake = FakeBackend::new();
        configure(&mut app, &mut fake);
        assert_eq!(app.messages, fake.messages);
        assert!(matches!(Source::Demo(fake), Source::Demo(_)));
        let image = demo_image_view(&demo_dot(), IMAGE_ID).unwrap();
        assert_eq!(image.preview_dimensions(), Some((1200, 200)));
        assert_eq!(
            image.source_path.as_ref().unwrap().to_str(),
            Some("quiet-horizon.png")
        );
        let (tx, _) = std::sync::mpsc::channel();
        app.apply(
            crate::Reply::HistoryImage(
                "demo-room".into(),
                IMAGE_ID.into(),
                native_media::prepare(image),
            ),
            &tx,
        );
        app
    }

    #[test]
    fn showcase_timer_stays_at_one_second_and_new_operations_use_real_time() {
        let mut app = App::demo();
        let mut fake = FakeBackend::new();
        configure(&mut app, &mut fake);
        for elapsed in [0, 2, 60, 3600] {
            app.operation.as_mut().unwrap().1 =
                std::time::Instant::now() - std::time::Duration::from_secs(elapsed);
            assert_eq!(app.status_line(), "读取消息 · 1秒");
        }
        app.begin("发送消息");
        app.operation.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_secs(5);
        assert_eq!(app.status_line(), "发送消息 · 5秒");
        let mut normal = App::demo();
        normal.begin("读取消息");
        normal.operation.as_mut().unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_secs(8);
        assert_eq!(normal.status_line(), "读取消息 · 8秒");
    }

    #[test]
    fn all_highlights_fit_in_128_by_40_without_scrolling() {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        let app = scene();
        let area = Rect::new(0, 0, 128, 40);
        ui::settle(&app, area);
        let timeline = ui::timeline(&app, area);
        assert_eq!(
            timeline.limit(),
            0,
            "{} rows in {}",
            timeline.total,
            timeline.content.height
        );
        let mut terminal = Terminal::new(TestBackend::new(128, 40)).unwrap();
        terminal.draw(|f| ui::draw(f, &app)).unwrap();
        let frame = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        for text in [
            "dot-tui",
            "quiet horizon",
            "A little calm",
            "fn main()",
            "quiet-horizon.png",
            "Could you make",
            "Ctrl+Q",
            "Reading messages",
        ] {
            assert!(frame.contains(text), "missing {text}");
        }
        assert_eq!(
            app.messages
                .iter()
                .filter(|m| !m.created_at.is_empty())
                .count(),
            2
        );
        for label in ["offline demo", "showcase", "开发模拟", "演示"] {
            assert!(
                !frame.contains(label),
                "showcase-only UI label leaked: {label}"
            );
        }
        assert_eq!(
            app.operation.as_ref().map(|(phase, _)| phase.as_str()),
            Some("读取消息")
        );
        for message in &app.messages {
            let timestamp = chrono::DateTime::parse_from_rfc3339(&message.created_at)
                .unwrap()
                .with_timezone(&chrono::Local)
                .format("%-I:%M %p")
                .to_string();
            assert!(
                frame.contains(&timestamp),
                "timestamp is visible: {timestamp}"
            );
        }
        assert!(
            !app.history_frame.borrow().is_empty(),
            "actual inline media placement"
        );
    }

    #[test]
    fn held_refresh_does_not_poll_and_interaction_uses_only_fake_backend() {
        let mut app = App::demo();
        let mut fake = FakeBackend::new();
        configure(&mut app, &mut fake);
        let (tx, rx) = std::sync::mpsc::channel();
        crate::poll_latest(&mut app, &tx);
        assert!(rx.try_recv().is_err());
        crate::key(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &tx,
        );
        let job = rx.try_recv().expect("interactive send stays local");
        assert!(matches!(&job, crate::Job::Send(..)));
        let mut source = Source::Demo(fake);
        app.apply(source.perform(job), &tx);
        let page = rx.try_recv().expect("read back the fake send");
        app.apply(source.perform(page), &tx);
        assert_eq!(app.messages.len(), 4);
        assert!(app.messages.iter().any(|m| m.text.contains("离线模拟回复")));
        assert!(!app.live);
        assert!(crate::key(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::CONTROL
            ),
            &tx,
        ));
    }

    #[test]
    fn fallback_and_resize_keep_the_composer_and_footer() {
        for protocol in [
            crate::terminal::image::GraphicsProtocol::None,
            crate::terminal::image::GraphicsProtocol::Kitty,
        ] {
            let _guard = crate::terminal::image::set_protocol_for_test(protocol);
            let app = scene();
            for (w, h) in [(80, 24), (42, 18), (128, 40), (160, 48)] {
                let area = Rect::new(0, 0, w, h);
                ui::settle(&app, area);
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| ui::draw(f, &app)).unwrap();
                assert!(ui::prompt_hit_area(&app, area).bottom() < h);
            }
        }
    }
}
