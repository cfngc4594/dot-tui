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
// Original OtherToolCallBlock::output middle truncation, tool/other.rs
// 182–205 at 2bdd1d6. Dot metadata supplies a filename rather than a local path.
fn media_name_display(path_str: String, max_w: usize) -> String {
    if path_str.chars().count() > max_w {
        let keep = max_w.saturating_sub(3) / 2;
        let end_keep = max_w.saturating_sub(3) - keep;
        let chars: Vec<char> = path_str.chars().collect();
        match (
            chars.get(..keep),
            chars
                .len()
                .checked_sub(end_keep)
                .and_then(|i| chars.get(i..)),
        ) {
            (Some(head), Some(tail)) => format!(
                "{}...{}",
                head.iter().collect::<String>(),
                tail.iter().collect::<String>()
            ),
            _ => path_str,
        }
    } else {
        path_str
    }
}
pub fn history_lines(
    file: &str,
    info: Option<&Info>,
    width: u16,
    inline: bool,
) -> (Vec<Line<'static>>, u16, Option<u16>) {
    let (mut lines, _) = attachment_lines(0, width);
    // Retain Dot's existing compact presentation. Public Grok 2bdd1d6 is
    // older than installed 1.0.77 and is not evidence for removing its UI.
    lines[0] = Line::styled(
        media_name_display(
            info.map_or("Image".into(), |i| i.name.clone()),
            width as usize,
        ),
        Style::default()
            .fg(Color::Rgb(225, 225, 225))
            .add_modifier(Modifier::BOLD),
    );
    lines[1] = Line::from("");
    // Dot receives remote readiness/error metadata instead of Grok's local
    // decoded InlineMediaInfo. Never reserve an image region until both
    // dimensions are valid and the decode/download state is successful.
    let ready = info.filter(|i| i.error.is_none() && i.dimensions.0 > 0 && i.dimensions.1 > 0);
    if !inline || ready.is_none() {
        let title = lines.remove(0);
        let open = lines.remove(2);
        return (vec![title, open], 1, None);
    }
    let _ = file; // Remote identifiers are never shown as user-facing filenames.
    if inline && let Some(info) = ready {
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

#[cfg(test)]
mod image_fallback_tests {
    use super::*;
    #[test]
    fn long_media_names_follow_original_middle_truncation() {
        assert_eq!(
            media_name_display("1234567890abcdefghij.png".into(), 13),
            "12345...j.png"
        );
        assert_eq!(
            media_name_display("中文文件名称很长的图.png".into(), 9),
            "中文文...png"
        );
        assert_eq!(media_name_display("image.png".into(), 80), "image.png");
    }
    #[test]
    fn fallback_retains_existing_compact_title_and_open() {
        let info = Info {
            name: "photo.png".into(),
            dimensions: (100, 100),
            error: None,
        };
        let (lines, button, rows) = history_lines("remote-id", Some(&info), 80, false);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), "photo.png");
        assert!(lines[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(
            lines[1]
                .to_string()
                .ends_with(media_open_button_label(false))
        );
        assert_eq!(button, 1);
        assert_eq!(rows, None);
    }
    #[test]
    fn unavailable_or_failed_remote_images_never_reserve_blank_pixels() {
        for (dimensions, error) in [
            ((0, 0), None),
            ((0, 100), None),
            ((100, 0), None),
            ((100, 100), Some("decode failed".into())),
        ] {
            let info = Info {
                name: "image.png".into(),
                dimensions,
                error,
            };
            let (lines, button, rows) = history_lines("file", Some(&info), 80, true);
            assert_eq!(lines.len(), 2);
            assert_eq!(button, 1);
            assert_eq!(rows, None);
        }
        let info = Info {
            name: "image.png".into(),
            dimensions: (100, 100),
            error: None,
        };
        let (lines, _, rows) = history_lines("file", Some(&info), 80, false);
        assert_eq!(lines.len(), 2);
        assert_eq!(rows, None);
        assert!(history_lines("file", Some(&info), 80, true).2.is_some());
    }
}
