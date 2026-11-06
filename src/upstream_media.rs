//! Grok 2bdd1d6, Apache-2.0: scrollback/render.rs media_open_button_* and
//! blocks/tool/other.rs button lines; image sizing from inline_media_ffmpeg.rs.
//! Dot adapts room metadata/download state, retaining upstream image helpers.
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;
pub fn media_open_button_label(is_video: bool) -> &'static str {
    if is_video {
        crate::i18n::label("[Open Video]", "[打开视频]")
    } else {
        crate::i18n::label("[Open Image]", "[打开图片]")
    }
}
pub fn media_open_button_col(content_width: u16, is_video: bool) -> u16 {
    let label_w = media_open_button_label(is_video).width() as u16;
    content_width.saturating_sub(label_w) / 2
}
pub fn attachment_lines(
    _index: usize,
    content_width: u16,
) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let label = media_open_button_label(false);
    let col = media_open_button_col(content_width, false);
    let open_line = Line::from(vec![
        Span::raw(" ".repeat(col as usize)),
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(Color::Rgb(58, 149, 171))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let lines = vec![
        Line::styled(
            "Image",
            Style::default()
                .fg(Color::Rgb(225, 225, 225))
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled("", Style::default().fg(Color::Rgb(88, 88, 88))),
        Line::from(""),
        open_line,
        Line::from(""),
    ];
    (
        lines,
        (content_width >= label.width() as u16).then_some((col, label.width() as u16)),
    )
}

// Grok inline_media_ffmpeg.rs::inline_media_reserved_rows, image branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    pub dimensions: (u32, u32),
    pub error: Option<String>,
}
pub fn reserved_rows(info: &Info, width: u16) -> u16 {
    crate::terminal::image::fit_image_to_cells(
        info.dimensions.0,
        info.dimensions.1,
        width.saturating_sub(2),
        (width / 2).clamp(4, 20),
    )
    .1
}
pub fn history_lines(
    file: &str,
    info: Option<&Info>,
    width: u16,
    inline: bool,
) -> (Vec<Line<'static>>, u16, Option<u16>) {
    let (mut lines, _) = attachment_lines(0, width);
    lines[0] = Line::styled(
        info.map_or("Image".into(), |i| i.name.clone()),
        Style::default()
            .fg(Color::Rgb(225, 225, 225))
            .add_modifier(Modifier::BOLD),
    );
    // Progress and error details belong to the single top-right status surface.
    // Keep the media action; failed/unprepared media reserves no blank image box.
    lines[1] = Line::from("");
    if !inline || info.is_none_or(|i| i.dimensions == (0, 0)) {
        let title = lines.remove(0);
        let open = lines.remove(2);
        return (vec![title, open], 1, None);
    }
    let _ = file; // Remote identifiers are never shown as user-facing filenames.
    if inline && let Some(info) = info.filter(|i| i.dimensions.0 > 0 && i.dimensions.1 > 0) {
        let rows = reserved_rows(info, width);
        let button = lines.remove(3);
        lines.truncate(3);
        lines.extend((0..rows).map(|_| Line::from("")));
        lines.push(Line::from(""));
        lines.push(button);
        return (lines, rows + 4, Some(rows));
    }
    (lines, 3, None)
}
