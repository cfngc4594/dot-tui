//! Fixed-theme terminal adapter for original image/overlay modules.
#[allow(dead_code)]
pub mod image;
#[allow(dead_code)]
pub mod overlay;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalName {
    Kitty,
    Ghostty,
    WezTerm,
    WarpTerminal,
    Iterm2,
    AppleTerminal,
    Unknown,
}
pub struct Context {
    pub brand: TerminalName,
    pub term_features: Option<String>,
    pub is_ssh: bool,
    skip: bool,
}
impl Context {
    pub fn graphics_protocol_skip_reason(&self) -> Option<&'static str> {
        self.skip.then_some("unsupported terminal surface")
    }
}
pub fn terminal_context() -> Context {
    use std::io::IsTerminal;
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let brand = match program.as_str() {
        "ghostty" => TerminalName::Ghostty,
        "WezTerm" => TerminalName::WezTerm,
        "WarpTerminal" => TerminalName::WarpTerminal,
        "iTerm.app" => TerminalName::Iterm2,
        "Apple_Terminal" => TerminalName::AppleTerminal,
        _ => {
            if std::env::var_os("KITTY_WINDOW_ID").is_some()
                || std::env::var("TERM").is_ok_and(|v| v.contains("kitty"))
            {
                TerminalName::Kitty
            } else {
                TerminalName::Unknown
            }
        }
    };
    Context {
        brand,
        term_features: std::env::var("TERM_FEATURES").ok(),
        is_ssh: std::env::var_os("SSH_CONNECTION").is_some(),
        skip: !std::io::stdout().is_terminal()
            || std::env::var_os("TMUX").is_some()
            || std::env::var_os("STY").is_some(),
    }
}
