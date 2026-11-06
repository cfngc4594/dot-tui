// Grok Build, Copyright 2023–2026 SpaceXAI, Apache-2.0.
// Unchanged CursorState/CursorAction from pager-render/src/render/draw.rs
// commit 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
use crossterm::{QueueableCommand, cursor};
use std::io::Write;
#[derive(Debug, Default)]
pub struct CursorState {
    /// `None` means the cursor is hidden; `Some((x, y))` means it is visible at (x, y).
    last_pos: Option<(u16, u16)>,
}
/// What cursor commands to emit after a frame render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorAction {
    /// No cursor commands needed, so the blink timer is preserved.
    None,
    /// Cursor is visible and cells changed: reposition after cell writes disturbed the terminal cursor.
    /// Resets blink (unavoidable when cells change on screen).
    Reposition(u16, u16),
    /// Cursor becoming visible at (x, y); needs `MoveTo` and `Show`.
    Show(u16, u16),
    /// Cursor becoming hidden; needs `Hide`.
    Hide,
}
impl CursorState {
    pub fn new() -> Self {
        Self { last_pos: None }
    }
    /// Determine what cursor action to take for this frame.
    ///
    /// No side effects; call [`apply`] to execute the returned action.
    pub fn action(&self, cursor_pos: Option<(u16, u16)>, has_changes: bool) -> CursorAction {
        if cursor_pos == self.last_pos {
            if has_changes && let Some((x, y)) = cursor_pos {
                return CursorAction::Reposition(x, y);
            }
            CursorAction::None
        } else {
            match (cursor_pos, self.last_pos) {
                (Some((x, y)), Some(_)) => CursorAction::Reposition(x, y),
                (Some((x, y)), None) => CursorAction::Show(x, y),
                (None, Some(_)) => CursorAction::Hide,
                (None, None) => CursorAction::None,
            }
        }
    }
    /// `queue!` not `execute!`: cursor commands must batch with the frame and flush atomically on the writer thread.
    pub fn apply<W: Write>(&mut self, action: CursorAction, w: &mut W) {
        match action {
            CursorAction::None => {}
            CursorAction::Reposition(x, y) => {
                let _ = w.queue(cursor::MoveTo(x, y));
                self.last_pos = Some((x, y));
            }
            CursorAction::Show(x, y) => {
                let _ = w.queue(cursor::MoveTo(x, y));
                let _ = w.queue(cursor::Show);
                self.last_pos = Some((x, y));
            }
            CursorAction::Hide => {
                let _ = w.queue(cursor::Hide);
                self.last_pos = None;
            }
        }
    }
}
