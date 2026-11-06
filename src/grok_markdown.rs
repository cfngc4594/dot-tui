// Grok Build renderer integration. Copyright 2023-2026 SpaceXAI for the
// original palette/style/wrapping code. SPDX-License-Identifier: Apache-2.0
// Upstream: https://github.com/xai-org/grok-build
// Verified commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8
// Original parser, renderer, syntax engine and assets are in vendor/.
// Dot-specific changes: safe input/output boundary, bounded render cache,
// fixed GrokNight theme selection, and a small Lines-only integration API.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use xai_grok_markdown::{MarkdownBuffers, Syntect};

mod line_utils;
mod md_style;
#[allow(dead_code)]
mod wrapping;

struct CachedRender {
    text: String,
    width: usize,
    lines: Vec<Line<'static>>,
    bytes: usize,
}

const CACHE_MAX_ENTRIES: usize = 288;
const CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

impl CachedRender {
    fn new(text: String, width: usize, lines: Vec<Line<'static>>) -> Self {
        // Count source and rendered output, not just source length. Tiny-width
        // wrapping and syntax highlighting can create many Line/Span objects.
        let bytes = std::mem::size_of::<Self>()
            + text.capacity()
            + lines.capacity() * std::mem::size_of::<Line<'static>>()
            + lines
                .iter()
                .map(|line| {
                    line.spans.capacity() * std::mem::size_of::<ratatui::text::Span<'static>>()
                        + line
                            .spans
                            .iter()
                            .map(|span| match &span.content {
                                std::borrow::Cow::Owned(text) => text.capacity(),
                                std::borrow::Cow::Borrowed(_) => 0,
                            })
                            .sum::<usize>()
                })
                .sum::<usize>();
        Self {
            text,
            width,
            lines,
            bytes,
        }
    }
}

fn cache_insert(
    cache: &mut VecDeque<CachedRender>,
    entry: CachedRender,
    max_entries: usize,
    max_bytes: usize,
) {
    // Do not evict useful history to retain one disproportionately large item.
    if max_entries == 0 || entry.bytes > max_bytes {
        return;
    }
    let mut bytes = cache.iter().map(|entry| entry.bytes).sum::<usize>();
    while cache.len() >= max_entries || bytes + entry.bytes > max_bytes {
        if let Some(removed) = cache.pop_front() {
            bytes -= removed.bytes;
        } else {
            break;
        }
    }
    cache.push_back(entry);
}

thread_local! {
    // Rendering a stable history on every frame should not reparse/highlight it.
    // Cover the full bounded history, not just half of a default 32-message
    // page, which would evict itself on every frame. Both count and bytes cap it.
    static CACHE: RefCell<VecDeque<CachedRender>> = const { RefCell::new(VecDeque::new()) };
    #[cfg(test)]
    static PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Render with Grok Build's actual markdown parser, renderer, syntax highlighter,
/// GrokNight markdown styles and span-preserving word wrap.
pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let width = width.min(4096);
    let text = crate::sanitize::strip_control_chars(text, true);
    if let Some(lines) = CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|entry| entry.width == width && entry.text == text)
            .map(|entry| entry.lines.clone())
    }) {
        return lines;
    }

    #[cfg(test)]
    PARSE_COUNT.with(|count| count.set(count.get() + 1));

    static SYNTAX: OnceLock<Syntect> = OnceLock::new();
    let syntax = SYNTAX.get_or_init(|| {
        Syntect::new(include_bytes!(
            "../vendor/xai-grok-markdown/assets/tokyo-night.tmTheme"
        ))
    });
    let mut buffers = MarkdownBuffers::new();
    let (output, _) = xai_grok_markdown::render_markdown_ratatui_with_buffers_width(
        &text,
        md_style::style(),
        true,
        &mut buffers,
        Some(syntax),
        Some(width),
    );

    // Markdown entities are decoded inside the original renderer. Sanitize a
    // second time so encoded bidi/terminal controls cannot bypass the boundary.
    let safe_lines = output.lines.into_iter().map(|mut line| {
        for span in &mut line.spans {
            span.content = crate::sanitize::strip_control_chars(&span.content, false).into();
        }
        line
    });
    let lines = wrap_bounded(safe_lines, width);
    CACHE.with(|cache| {
        cache_insert(
            &mut cache.borrow_mut(),
            CachedRender::new(text, width, lines.clone()),
            CACHE_MAX_ENTRIES,
            CACHE_MAX_BYTES,
        );
    });
    lines
}

/// Use Grok's actual styled word-wrapper without interpreting user input as
/// markdown. Empty input and trailing newlines retain their visible input rows.
pub fn wrap_plain(text: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let text = crate::sanitize::strip_control_chars(text, true);
    wrap_bounded(
        text.split('\n').map(|line| Line::from(line.to_owned())),
        width.min(4096),
    )
}

// Shared upstream word wrapper for styled history chips and collapsed tails.
pub(crate) fn wrap_styled(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    wrap_bounded(lines, width.min(4096))
}

fn wrap_bounded(
    lines: impl IntoIterator<Item = Line<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    wrapping::word_wrap_lines(lines, width)
        .into_iter()
        .map(|line| {
            // A 2-cell glyph cannot fit a 1-cell viewport. Keep the renderer within
            // its viewport, including these pathological narrow-terminal cases.
            if line.width() > width {
                line_utils::fit_line_to_width(line, width)
            } else {
                line
            }
        })
        .collect()
}

// Selected fields from crates/codegen/xai-grok-pager-render/src/theme/groknight.rs.
// Palette values and heading modifiers are copied exactly. Runtime theme
// detection and the remainder of the pager Theme object are not included.
struct MarkdownTheme {
    md_heading_h1: Color,
    md_heading_h2: Color,
    md_heading_h3: Color,
    md_heading_h4: Color,
    md_heading_h5: Color,
    md_heading_h6: Color,
    md_heading_h1_mod: Modifier,
    md_heading_h2_mod: Modifier,
    md_heading_h3_mod: Modifier,
    md_heading_h4_mod: Modifier,
    md_heading_h5_mod: Modifier,
    md_heading_h6_mod: Modifier,
    md_text: Color,
    md_code: Color,
    md_muted: Color,
    md_code_bg: Color,
    md_task_checked: Color,
    md_task_unchecked: Color,
    link_fg: Color,
}

fn theme() -> MarkdownTheme {
    MarkdownTheme {
        md_heading_h1: Color::Rgb(26, 188, 156),
        md_heading_h2: Color::Rgb(122, 162, 247),
        md_heading_h3: Color::Rgb(157, 124, 216),
        md_heading_h4: Color::Rgb(120, 120, 120),
        md_heading_h5: Color::Rgb(108, 108, 108),
        md_heading_h6: Color::Rgb(90, 90, 90),
        md_heading_h1_mod: Modifier::BOLD,
        md_heading_h2_mod: Modifier::BOLD,
        md_heading_h3_mod: Modifier::BOLD,
        md_heading_h4_mod: Modifier::BOLD,
        md_heading_h5_mod: Modifier::BOLD,
        md_heading_h6_mod: Modifier::empty(),
        md_text: Color::Rgb(200, 200, 200),
        md_code: Color::Rgb(58, 149, 171),
        md_muted: Color::Rgb(108, 108, 108),
        md_code_bg: Color::Rgb(28, 28, 28),
        md_task_checked: Color::Rgb(158, 206, 106),
        md_task_unchecked: Color::Rgb(200, 200, 200),
        link_fg: Color::Rgb(122, 166, 218),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn has_style(lines: &[Line<'_>], text: &str, modifier: Modifier) -> bool {
        lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content.contains(text)
                    && line.style.patch(span.style).add_modifier.contains(modifier)
            })
        })
    }

    #[test]
    fn headings_emphasis_and_inline_code_use_original_styles() {
        let lines = render("## Heading\n\n**bold** and *italic* and `code`", 70);
        let text = plain(&lines);
        assert!(text.contains("Heading"));
        assert!(!text.contains("##"));
        assert!(!text.contains("**"));
        assert!(!text.contains('`'));
        assert!(has_style(&lines, "Heading", Modifier::BOLD));
        assert!(has_style(&lines, "bold", Modifier::BOLD));
        assert!(has_style(&lines, "italic", Modifier::ITALIC));
        assert!(has_style(&lines, "code", Modifier::BOLD));
    }

    #[test]
    fn lists_and_links_are_rendered() {
        let lines = render("- First\n- [OpenAI](https://openai.com)", 80);
        let text = plain(&lines);
        assert!(text.contains("• First"), "{text}");
        assert!(!text.contains("[OpenAI]"));
        assert!(has_style(&lines, "OpenAI", Modifier::UNDERLINED));
    }

    #[test]
    fn code_fences_are_hidden_and_background_survives_wrapping() {
        let lines = render("```rust\nlet example = 42;\n```", 12);
        let text = plain(&lines);
        assert!(!text.contains("```"));
        assert_eq!(
            text.split_whitespace().collect::<Vec<_>>().join(" "),
            "let example = 42;"
        );
        assert!(lines.iter().all(|line| line.width() <= 12));
        assert!(lines.iter().any(|line| {
            line.style.bg == Some(Color::Rgb(28, 28, 28))
                || line
                    .spans
                    .iter()
                    .any(|span| span.style.bg == Some(Color::Rgb(28, 28, 28)))
        }));
        let syntax_colors = lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter_map(|span| span.style.fg)
            .collect::<std::collections::HashSet<_>>();
        // Respect the upstream detector's cached, process-wide capability.
        // Tests must not mutate the environment or force a color-level cap:
        // either would race parallel tests and bypass the NO_COLOR contract.
        let color_level = xai_grok_markdown::get_color_level();
        if color_level.has_color() {
            // Basic ANSI may quantize these tokens to one color. Only the
            // richer palettes guarantee distinct colors for this example.
            if color_level.has_256() {
                assert!(
                    syntax_colors.len() >= 2,
                    "expected original syntect token colors"
                );
            }
        } else {
            assert!(
                syntax_colors.len() <= 1,
                "monochrome mode must not force token colors"
            );
        }
    }

    #[test]
    fn styled_wrapping_and_quote_continuations_are_preserved() {
        let lines = render("> **one two three four five six seven eight nine ten**", 14);
        assert!(lines.iter().all(|line| line.width() <= 14));
        let populated: Vec<_> = lines.iter().filter(|line| !line.spans.is_empty()).collect();
        assert!(populated.len() > 1);
        assert!(
            populated
                .iter()
                .all(|line| plain(&[(*line).clone()]).starts_with("│ "))
        );
        assert!(has_style(&lines, "one", Modifier::BOLD));
        assert!(has_style(&lines, "ten", Modifier::BOLD));
    }

    #[test]
    fn table_rows_fit_the_requested_viewport() {
        let lines = render("| Name | Value |\n| --- | --- |\n| 中文测试 | 42 |", 24);
        assert!(plain(&lines).contains("42"));
        assert!(lines.iter().all(|line| line.width() <= 24));
    }

    #[test]
    fn raw_and_entity_encoded_terminal_controls_are_safe() {
        let lines = render("safe\x1b]52;c;bad\x07\u{202e} **x** &#x202e; &#x1b;", 80);
        for span in lines.iter().flat_map(|line| &line.spans) {
            assert!(!span.content.chars().any(char::is_control));
            assert!(!span.content.contains('\u{202e}'));
        }
    }

    #[test]
    fn original_double_tilde_only_rule_is_retained() {
        let lines = render("~**10%** and ~~gone~~", 80);
        assert!(plain(&lines).contains('~'));
        assert!(has_style(&lines, "10%", Modifier::BOLD));
        assert!(!has_style(&lines, "10%", Modifier::CROSSED_OUT));
        assert!(has_style(&lines, "gone", Modifier::CROSSED_OUT));
    }

    #[test]
    fn plain_wrapper_keeps_markers_unicode_and_empty_rows() {
        let lines = wrap_plain("**中文** hello world\n", 12);
        assert!(plain(&lines).contains("**中文**"));
        assert!(lines.iter().all(|line| line.width() <= 12));
        assert_eq!(lines.last().unwrap().width(), 0);
        assert_eq!(wrap_plain("", 8).len(), 1);
        assert!(render("中文", 0).is_empty());
        assert!(render("中文", 1).iter().all(|line| line.width() <= 1));
    }

    #[test]
    fn full_default_message_page_stays_cached_on_second_render() {
        CACHE.with(|cache| cache.borrow_mut().clear());
        PARSE_COUNT.with(|count| count.set(0));
        let messages: Vec<_> = (0..32)
            .map(|i| format!("Message {i}: **bold** text and `code`."))
            .collect();
        for message in &messages {
            render(message, 72);
        }
        PARSE_COUNT.with(|count| assert_eq!(count.get(), messages.len()));
        for message in &messages {
            render(message, 72);
        }
        PARSE_COUNT.with(|count| {
            assert_eq!(
                count.get(),
                messages.len(),
                "the second full-page render must not parse any message again"
            )
        });
        CACHE.with(|cache| assert_eq!(cache.borrow().len(), messages.len()));
    }

    #[test]
    fn cache_bounds_count_and_rendered_output_bytes() {
        fn small(text: &str) -> CachedRender {
            CachedRender::new(text.to_owned(), 80, vec![Line::from("hello".to_owned())])
        }
        let mut cache = VecDeque::new();
        let budget = small("one").bytes * 2;
        for key in ["one", "two", "tri"] {
            cache_insert(&mut cache, small(key), 3, budget);
        }
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.front().unwrap().text, "two");
        assert!(cache.iter().map(|entry| entry.bytes).sum::<usize>() <= budget);

        cache_insert(&mut cache, small("for"), 1, budget);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.front().unwrap().text, "for");

        // A short source with huge rendered output must not bypass the budget.
        let huge = CachedRender::new(
            "big".to_owned(),
            80,
            vec![Line::from("z".repeat(budget * 2))],
        );
        assert!(huge.bytes > budget);
        cache_insert(&mut cache, huge, 3, budget);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.front().unwrap().text, "for");
    }
}
