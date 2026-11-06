//! Explicit keyboard protocol session; no TERM/brand guess authorizes sends.
use crossterm::{
    event::{
        KeyboardEnhancementFlags as Flags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
};
use std::io;
pub struct Session {
    pushed: bool,
    pub confirmed: bool,
}
impl Session {
    pub fn negotiate() -> Self {
        let mut session = Self {
            pushed: false,
            confirmed: false,
        };
        #[cfg(unix)]
        {
            if !matches!(
                crossterm::terminal::keyboard_enhancement_flags(),
                Ok(Some(_))
            ) {
                return session;
            }
            let requested = Flags::DISAMBIGUATE_ESCAPE_CODES
                | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                | Flags::REPORT_EVENT_TYPES;
            if execute!(io::stdout(), PushKeyboardEnhancementFlags(requested)).is_err() {
                return session;
            }
            session.pushed = true;
            session.confirmed = matches!(crossterm::terminal::keyboard_enhancement_flags(),Ok(Some(actual)) if actual.contains(requested));
        }
        session
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.pushed {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
    }
}
