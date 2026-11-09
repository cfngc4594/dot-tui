//! Original GrokNight palette subset for extracted image/dropdown widgets.
use ratatui::style::{Color, Style};
#[allow(dead_code)]
pub struct Theme {
    pub bg_base: Color,
    pub selection_border: Color,
    pub bg_dark: Color,
    pub bg_light: Color,
    pub bg_hover: Color,
    pub bg_visual: Color,
    pub gray: Color,
    pub gray_dim: Color,
    pub text_primary: Color,
    pub fuzzy_accent: Color,
    pub accent_success: Color,
    pub paste_bg: Color,
    pub paste_fg: Color,
    pub paste_dim: Color,
}
impl Theme {
    /// Original panel_border_fg on the fixed RGB GrokNight palette.
    pub fn panel_border_fg(&self) -> Color {
        self.bg_light
    }
    pub fn current() -> Self {
        Self {
            bg_base: Color::Rgb(20, 20, 20),
            selection_border: Color::Rgb(60, 60, 65),
            bg_dark: Color::Rgb(28, 28, 28),
            bg_light: Color::Rgb(36, 36, 36),
            bg_hover: Color::Rgb(44, 44, 44),
            bg_visual: Color::Rgb(54, 54, 54),
            gray: Color::Rgb(108, 108, 108),
            gray_dim: Color::Rgb(88, 88, 88),
            text_primary: Color::Rgb(225, 225, 225),
            fuzzy_accent: Color::Rgb(122, 162, 247),
            accent_success: Color::Rgb(158, 206, 106),
            paste_bg: Color::Rgb(17, 17, 17),
            paste_fg: Color::Rgb(200, 200, 200),
            paste_dim: Color::Rgb(65, 65, 65),
        }
    }
    #[allow(dead_code)]
    pub fn selection_overlay(&self) -> Style {
        Style::default()
    }
    #[allow(dead_code)]
    pub fn hover_overlay(&self) -> Style {
        Style::default()
    }
}
