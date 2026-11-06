//! Isolated synthetic-only harness for the cropped original Grok image widgets.
//! No clipboard, file selection, auth, dot backend, or transport is installed.
use crate::{image_composer::ImageComposer, terminal::overlay};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend, style::Color};
use std::{
    io::{self, Write},
    time::Duration,
};

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if let Some(clear) = overlay::clear() {
            let _ = overlay::PostFlush::from(clear).write_to(&mut out);
        }
        let _ = execute!(out, crossterm::cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        let _ = out.flush();
    }
}
pub fn run() -> Result<(), String> {
    crate::require_terminal(
        std::io::IsTerminal::is_terminal(&io::stdin()),
        std::io::IsTerminal::is_terminal(&io::stdout()),
    )?;
    let mut bytes = io::Cursor::new(Vec::new());
    let image = image::ImageBuffer::from_fn(160, 100, |x, y| {
        image::Rgb([(x * 255 / 160) as u8, (y * 255 / 100) as u8, 128])
    });
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|_| "synthetic encoding failed")?;
    let mut composer = ImageComposer::default();
    composer.stage_bytes(bytes.into_inner(), None)?;
    let image = &composer.images[0];
    image.prepare_preview_blocking();
    terminal::enable_raw_mode().map_err(|_| "raw mode unavailable")?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen, crossterm::cursor::Hide)
        .map_err(|_| "alternate screen unavailable")?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|_| "terminal unavailable")?;
    let mut open = true;
    loop {
        let mut escapes = None;
        execute!(terminal.backend_mut(), BeginSynchronizedUpdate)
            .map_err(|_| "terminal write failed")?;
        terminal.draw(|frame|{
   let area=frame.area();
   frame.render_widget(ratatui::widgets::Paragraph::new("Synthetic image module harness · q: exit · Esc: close · p: preview\nNo clipboard/account/network access"),area);
   if open {
    let (_,pixels)=crate::render::image_overlay::render_image_overlay_checked(frame.buffer_mut(),area,image,Color::Rgb(20,20,20),Color::Rgb(225,225,225),Color::Rgb(108,108,108));
    escapes=pixels;
   }else{escapes=overlay::clear();}
  }).map_err(|_|"draw failed")?;
        if let Some(escapes) = escapes {
            overlay::PostFlush::from(escapes)
                .write_to(terminal.backend_mut())
                .map_err(|_| "image write failed")?;
        }
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)
            .map_err(|_| "terminal write failed")?;
        terminal
            .backend_mut()
            .flush()
            .map_err(|_| "terminal flush failed")?;
        if event::poll(Duration::from_millis(100)).map_err(|_| "input unavailable")? {
            match event::read().map_err(|_| "input unavailable")? {
                Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => open = false,
                    KeyCode::Char('p') => open = true,
                    _ => {}
                },
                Event::Resize(_, _) => {
                    terminal.autoresize().map_err(|_| "resize failed")?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
