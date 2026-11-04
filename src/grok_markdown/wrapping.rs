// Copyright 2023-2026 SpaceXAI. SPDX-License-Identifier: Apache-2.0
// Grok Build commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8
// Source: crates/codegen/xai-grok-pager-render/src/render/wrapping.rs
// Production source unchanged. Pager-specific test module omitted.
//! Provides word-aware wrapping that preserves styled spans and tracks soft vs hard line breaks (via joiners).

use ratatui::text::{Line, Span};
use std::borrow::Cow;
use std::ops::Range;
use textwrap::Options;
use unicode_width::UnicodeWidthChar;

use super::line_utils::{fit_line_to_width, push_owned_lines};

/// Compute byte ranges for wrapped lines without trailing whitespace.
pub(crate) fn wrap_ranges_trim<'a, O>(text: &str, width_or_options: O) -> Vec<Range<usize>>
where
    O: Into<Options<'a>>,
{
    let opts = width_or_options.into();
    let mut lines: Vec<Range<usize>> = Vec::new();
    for line in textwrap::wrap(text, opts).iter() {
        match line {
            Cow::Borrowed(slice) => {
                let start = unsafe { slice.as_ptr().offset_from(text.as_ptr()) as usize };
                let end = start + slice.len();
                lines.push(start..end);
            }
            Cow::Owned(_) => panic!("wrap_ranges_trim: unexpected owned string"),
        }
    }
    lines
}

/// A segment of a match highlight on a single wrapped row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSegment {
    /// Wrapped row index (0-based).
    pub row: usize,
    /// Start display column within that row (0-based, unicode-width aware).
    pub col_start: usize,
    /// End display column (exclusive).
    pub col_end: usize,
}

/// One segment per wrapped row the match overlaps. Columns are display width, not byte offsets.
pub fn byte_range_to_row_cols(
    text: &str,
    wrap_ranges: &[Range<usize>],
    match_range: Range<usize>,
) -> Vec<HighlightSegment> {
    let mut segments = Vec::new();
    for (row, wr) in wrap_ranges.iter().enumerate() {
        // Intersect match_range with this row's byte range.
        let start = match_range.start.max(wr.start);
        let end = match_range.end.min(wr.end);
        if start < end {
            // Convert byte offsets within this row to display columns.
            let Some(row_text) = text.get(wr.start..wr.end) else {
                continue;
            };
            let col_start = byte_offset_to_display_col(row_text, start - wr.start);
            let col_end = byte_offset_to_display_col(row_text, end - wr.start);
            segments.push(HighlightSegment {
                row,
                col_start,
                col_end,
            });
        }
    }
    segments
}

/// Convert a byte offset within `text` to the corresponding display column.
pub(crate) fn byte_offset_to_display_col(text: &str, byte_offset: usize) -> usize {
    let mut col = 0usize;
    let mut bytes = 0usize;
    for ch in text.chars() {
        if bytes >= byte_offset {
            break;
        }
        bytes += ch.len_utf8();
        col += ch.width().unwrap_or(0);
    }
    col
}

/// Breakpoints must match visual rendering. FirstFit is greedy, so single-pass equals the two-stage wrap; a non-greedy change must mirror it.
#[allow(clippy::single_range_in_vec_init)] // Intentional: the single range is the full text, no wrapping
pub fn wrap_byte_ranges_matching(text: &str, width: usize) -> Vec<Range<usize>> {
    if width == 0 || text.is_empty() {
        return vec![0..text.len()];
    }

    // Must match the Options used by word_wrap_line_with_joiners (via RtOptions).
    // Blockquote lines get a subsequent_indent equal to the prefix, which reduces the effective width for continuation lines
    // Mirror that here so search-highlight breakpoints stay in sync with the visual rendering
    let bq_len = blockquote_prefix_len(text);
    let subsequent_width = if bq_len > 0 && bq_len < text.len() {
        match text.get(..bq_len) {
            Some(prefix) => {
                let prefix_display_width = prefix
                    .chars()
                    .map(|c| c.width().unwrap_or(0))
                    .sum::<usize>();
                width.saturating_sub(prefix_display_width).max(1)
            }
            None => width,
        }
    } else {
        width
    };

    // First line wraps at full width; remainder at reduced width for blockquotes.
    let first_line_opts = textwrap::Options::new(width)
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
        .word_splitter(textwrap::WordSplitter::HyphenSplitter)
        .break_words(true);
    let first_ranges = wrap_ranges_trim(text, first_line_opts);
    let Some(first) = first_ranges.first() else {
        return vec![0..text.len()];
    };

    let mut ranges = vec![first.clone()];
    let mut base = first.end;

    // Skip whitespace at the wrap boundary (mirrors word_wrap_line_with_joiners).
    let skip = match text.get(base..) {
        Some(rest) => rest.chars().take_while(|c| *c == ' ').count(),
        None => 0,
    };
    base = base.saturating_add(skip);

    if base < text.len() {
        let Some(remainder) = text.get(base..) else {
            return ranges;
        };
        let rem_opts = textwrap::Options::new(subsequent_width)
            .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
            .word_splitter(textwrap::WordSplitter::HyphenSplitter)
            .break_words(true);
        for r in wrap_ranges_trim(remainder, rem_opts) {
            ranges.push((r.start + base)..(r.end + base));
        }
    }

    ranges
}

/// Wrapping options with initial/subsequent indent support.
#[derive(Debug, Clone)]
pub struct RtOptions<'a> {
    /// The width in columns at which the text will be wrapped.
    pub width: usize,
    pub line_ending: textwrap::LineEnding,
    pub initial_indent: Line<'a>,
    pub subsequent_indent: Line<'a>,
    /// Allow long words to be broken if they cannot fit on a line.
    pub break_words: bool,
    pub wrap_algorithm: textwrap::WrapAlgorithm,
    /// The line breaking algorithm to use.
    pub word_separator: textwrap::WordSeparator,
    pub word_splitter: textwrap::WordSplitter,
}

impl From<usize> for RtOptions<'_> {
    fn from(width: usize) -> Self {
        RtOptions::new(width)
    }
}

#[allow(dead_code)]
impl<'a> RtOptions<'a> {
    pub fn new(width: usize) -> Self {
        RtOptions {
            width,
            line_ending: textwrap::LineEnding::LF,
            initial_indent: Line::default(),
            subsequent_indent: Line::default(),
            break_words: true,
            word_separator: textwrap::WordSeparator::new(),
            wrap_algorithm: textwrap::WrapAlgorithm::FirstFit,
            word_splitter: textwrap::WordSplitter::HyphenSplitter,
        }
    }

    pub fn line_ending(self, line_ending: textwrap::LineEnding) -> Self {
        RtOptions {
            line_ending,
            ..self
        }
    }

    pub fn width(self, width: usize) -> Self {
        RtOptions { width, ..self }
    }

    pub fn initial_indent(self, initial_indent: Line<'a>) -> Self {
        RtOptions {
            initial_indent,
            ..self
        }
    }

    pub fn subsequent_indent(self, subsequent_indent: Line<'a>) -> Self {
        RtOptions {
            subsequent_indent,
            ..self
        }
    }

    pub fn break_words(self, break_words: bool) -> Self {
        RtOptions {
            break_words,
            ..self
        }
    }

    pub fn word_separator(self, word_separator: textwrap::WordSeparator) -> RtOptions<'a> {
        RtOptions {
            word_separator,
            ..self
        }
    }

    pub fn wrap_algorithm(self, wrap_algorithm: textwrap::WrapAlgorithm) -> RtOptions<'a> {
        RtOptions {
            wrap_algorithm,
            ..self
        }
    }

    pub fn word_splitter(self, word_splitter: textwrap::WordSplitter) -> RtOptions<'a> {
        RtOptions {
            word_splitter,
            ..self
        }
    }
}

/// Wrap a single line, preserving styles.
#[must_use]
pub fn word_wrap_line<'a, O>(line: &'a Line<'a>, width_or_options: O) -> Vec<Line<'a>>
where
    O: Into<RtOptions<'a>>,
{
    let (lines, _joiners) = word_wrap_line_with_joiners(line, width_or_options);
    lines
}

fn flatten_line_and_bounds<'a>(
    line: &'a Line<'a>,
) -> (String, Vec<(Range<usize>, ratatui::style::Style)>) {
    let mut flat = String::new();
    let mut span_bounds = Vec::new();
    let mut acc = 0usize;
    for s in &line.spans {
        let text = s.content.as_ref();
        let start = acc;
        flat.push_str(text);
        acc += text.len();
        span_bounds.push((start..acc, s.style));
    }
    (flat, span_bounds)
}

fn build_wrapped_line_from_range<'a>(
    indent: Line<'a>,
    original: &'a Line<'a>,
    span_bounds: &[(Range<usize>, ratatui::style::Style)],
    range: &Range<usize>,
    cursor: &mut usize,
) -> Line<'a> {
    let mut out = indent.style(original.style);
    let sliced = slice_line_spans(original, span_bounds, range, cursor);
    let mut spans = out.spans;
    spans.append(
        &mut sliced
            .spans
            .into_iter()
            .map(|s| s.patch_style(original.style))
            .collect(),
    );
    out.spans = spans;
    out
}

/// Never wrap tables: wrapping destroys column alignment. A leading `│` is a blockquote unless an interior `│` separates cells.
fn is_table_line(line: &Line<'_>) -> bool {
    let mut chars = line.spans.iter().flat_map(|s| s.content.chars());
    match chars.next() {
        // Box-drawing border characters (horizontal lines, corners, junctions): these are unambiguously table borders, never blockquote prefixes
        Some(ch)
            if ('\u{2500}'..='\u{257F}').contains(&ch) && ch != '\u{2502}' && ch != '\u{2503}' =>
        {
            true
        }
        // │ (U+2502) at line start is a table content row only if │ appears after the leading prefix region
        // Blockquote prefixes are only │ and spaces
        Some('\u{2502}') => {
            let mut in_prefix = true;
            for ch in chars {
                if in_prefix && (ch == '\u{2502}' || ch == ' ') {
                    continue;
                }
                in_prefix = false;
                if ch == '\u{2502}' {
                    return true;
                }
            }
            false
        }
        // ASCII table borders (TableBorders::ASCII uses '+' and '|')
        Some('|') => true,
        _ => false,
    }
}

/// Leading `│ ` sequences, else 0. Must agree with the selection layer's style-aware twin, which depends on re-injected prefix spans.
fn blockquote_prefix_len(flat: &str) -> usize {
    const BAR_BYTES: usize = '\u{2502}'.len_utf8(); // 3
    let mut len = 0;
    let mut chars = flat.chars();
    while let Some('\u{2502}') = chars.next() {
        if chars.next() == Some(' ') {
            len += BAR_BYTES + 1;
        } else {
            break;
        }
    }
    len
}

/// First line is `None`. Continuation joiners are the exact substring skipped at the wrap boundary (often spaces, possibly empty).
pub fn word_wrap_line_with_joiners<'a, O>(
    line: &'a Line<'a>,
    width_or_options: O,
) -> (Vec<Line<'a>>, Vec<Option<String>>)
where
    O: Into<RtOptions<'a>>,
{
    let mut rt_opts: RtOptions<'a> = width_or_options.into();

    // Never wrap tables. Pad to content width: terminal clipping desyncs on wider-than-measured glyphs and strands a ghost cell.
    if is_table_line(line) {
        let fitted = fit_line_to_width(line.clone(), rt_opts.width);
        return (vec![fitted], vec![None]);
    }

    let (flat, span_bounds) = flatten_line_and_bounds(line);

    // Blockquote lines start with │ (U+2502) prefix(es)
    // Set subsequent_indent to the prefix so continuation lines repeat the quote marker
    // Skip if the caller already set a subsequent_indent (caller intent wins).
    let bq_len = blockquote_prefix_len(&flat);
    if bq_len > 0 && bq_len < flat.len() && rt_opts.subsequent_indent.width() == 0 {
        let prefix = slice_line_spans(line, &span_bounds, &(0..bq_len), &mut 0);
        rt_opts = rt_opts.subsequent_indent(prefix);
    }

    let opts = Options::new(rt_opts.width)
        .line_ending(rt_opts.line_ending)
        .break_words(rt_opts.break_words)
        .wrap_algorithm(rt_opts.wrap_algorithm)
        .word_separator(rt_opts.word_separator)
        .word_splitter(rt_opts.word_splitter);

    let mut out: Vec<Line<'a>> = Vec::new();
    let mut joiners: Vec<Option<String>> = Vec::new();

    // The first output line uses the initial indent and a reduced available width.
    let initial_width_available = opts
        .width
        .saturating_sub(rt_opts.initial_indent.width())
        .max(1);
    let initial_wrapped = wrap_ranges_trim(&flat, opts.clone().width(initial_width_available));
    let Some(first_line_range) = initial_wrapped.first() else {
        out.push(rt_opts.initial_indent.clone());
        joiners.push(None);
        return (out, joiners);
    };

    // Rows are emitted in increasing byte order, so each row resumes the span scan where the previous one stopped instead of rescanning from 0
    // This shared cursor (see `slice_line_spans`) is what keeps wrapping one huge line linear
    let mut span_cursor = 0usize;

    let first_line = build_wrapped_line_from_range(
        rt_opts.initial_indent.clone(),
        line,
        &span_bounds,
        first_line_range,
        &mut span_cursor,
    );
    out.push(first_line);
    joiners.push(None);

    // Wrap the remainder using subsequent indent width.
    let mut base = first_line_range.end;
    let skip_leading_spaces = match flat.get(base..) {
        Some(rest) => rest.chars().take_while(|c| *c == ' ').count(),
        None => 0,
    };
    let joiner_end = base.saturating_add(skip_leading_spaces);
    let joiner_first = match flat.get(base..joiner_end) {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    base = joiner_end;

    let subsequent_width_available = opts
        .width
        .saturating_sub(rt_opts.subsequent_indent.width())
        .max(1);
    let Some(remaining) = flat.get(base..) else {
        return (out, joiners);
    };
    let remaining_wrapped = wrap_ranges_trim(remaining, opts.width(subsequent_width_available));

    let mut prev_end = 0usize;
    for (i, r) in remaining_wrapped.iter().enumerate() {
        if r.is_empty() {
            continue;
        }

        let joiner = if i == 0 {
            joiner_first.clone()
        } else {
            match remaining.get(prev_end..r.start) {
                Some(s) => s.to_string(),
                None => String::new(),
            }
        };
        prev_end = r.end;

        let offset_range = (r.start + base)..(r.end + base);
        let subsequent_line = build_wrapped_line_from_range(
            rt_opts.subsequent_indent.clone(),
            line,
            &span_bounds,
            &offset_range,
            &mut span_cursor,
        );
        out.push(subsequent_line);
        joiners.push(Some(joiner));
    }

    (out, joiners)
}

/// Wrap a sequence of lines, applying the initial indent only to the very first output line.
/// All later wrapped pieces use the subsequent indent.
#[allow(private_bounds)]
pub fn word_wrap_lines<'a, I, O, L>(lines: I, width_or_options: O) -> Vec<Line<'static>>
where
    I: IntoIterator<Item = L>,
    L: IntoLineInput<'a>,
    O: Into<RtOptions<'a>>,
{
    let base_opts: RtOptions<'a> = width_or_options.into();
    let mut out: Vec<Line<'static>> = Vec::new();

    for (idx, line) in lines.into_iter().enumerate() {
        let line_input = line.into_line_input();
        let opts = if idx == 0 {
            base_opts.clone()
        } else {
            let mut o = base_opts.clone();
            let sub = o.subsequent_indent.clone();
            o = o.initial_indent(sub);
            o
        };
        let wrapped = word_wrap_line(line_input.as_ref(), opts);
        push_owned_lines(&wrapped, &mut out);
    }

    out
}

/// Like `word_wrap_lines`, but also returns a parallel vector of soft-wrap joiners.
#[allow(private_bounds)]
pub fn word_wrap_lines_with_joiners<'a, I, O, L>(
    lines: I,
    width_or_options: O,
) -> (Vec<Line<'static>>, Vec<Option<String>>)
where
    I: IntoIterator<Item = L>,
    L: IntoLineInput<'a>,
    O: Into<RtOptions<'a>>,
{
    let base_opts: RtOptions<'a> = width_or_options.into();
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut joiners: Vec<Option<String>> = Vec::new();

    for (idx, line) in lines.into_iter().enumerate() {
        let line_input = line.into_line_input();
        let opts = if idx == 0 {
            base_opts.clone()
        } else {
            let mut o = base_opts.clone();
            let sub = o.subsequent_indent.clone();
            o = o.initial_indent(sub);
            o
        };

        let (wrapped, wrapped_joiners) = word_wrap_line_with_joiners(line_input.as_ref(), opts);
        for (l, j) in wrapped.into_iter().zip(wrapped_joiners) {
            out.push(super::line_utils::line_to_static(&l));
            joiners.push(j);
        }
    }

    (out, joiners)
}

/// Utilities to allow wrapping either borrowed or owned lines.
#[derive(Debug)]
enum LineInput<'a> {
    Borrowed(&'a Line<'a>),
    Owned(Line<'a>),
}

impl<'a> LineInput<'a> {
    fn as_ref(&self) -> &Line<'a> {
        match self {
            LineInput::Borrowed(line) => line,
            LineInput::Owned(line) => line,
        }
    }
}

/// This trait makes it easier to pass whatever we need into word_wrap_lines.
trait IntoLineInput<'a> {
    fn into_line_input(self) -> LineInput<'a>;
}

impl<'a> IntoLineInput<'a> for &'a Line<'a> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Borrowed(self)
    }
}

impl<'a> IntoLineInput<'a> for &'a mut Line<'a> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Borrowed(self)
    }
}

impl<'a> IntoLineInput<'a> for Line<'a> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(self)
    }
}

impl<'a> IntoLineInput<'a> for String {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(Line::from(self))
    }
}

impl<'a> IntoLineInput<'a> for &'a str {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(Line::from(self))
    }
}

impl<'a> IntoLineInput<'a> for Cow<'a, str> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(Line::from(self))
    }
}

impl<'a> IntoLineInput<'a> for Span<'a> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(Line::from(self))
    }
}

impl<'a> IntoLineInput<'a> for Vec<Span<'a>> {
    fn into_line_input(self) -> LineInput<'a> {
        LineInput::Owned(Line::from(self))
    }
}

/// Monotonic cursor so wrapping is O(rows + spans), not O(rows × spans) — that quadratic spun the render thread on one huge line.
/// Does not rewind: a later range must not start before a previous one.
fn slice_line_spans<'a>(
    original: &'a Line<'a>,
    span_bounds: &[(Range<usize>, ratatui::style::Style)],
    range: &Range<usize>,
    cursor: &mut usize,
) -> Line<'a> {
    let start_byte = range.start;
    let end_byte = range.end;

    // Spans ending at or before this row's start are done for every later row too (ranges are contiguous and queries monotonic)
    // So advance the shared cursor past them once and never revisit them
    while span_bounds
        .get(*cursor)
        .is_some_and(|(r, _)| r.end <= start_byte)
    {
        *cursor += 1;
    }

    let mut acc: Vec<Span<'a>> = Vec::new();
    for (i, (r, style)) in span_bounds.iter().enumerate().skip(*cursor) {
        let s = r.start;
        let e = r.end;
        if s >= end_byte {
            break;
        }
        let seg_start = start_byte.max(s);
        let seg_end = end_byte.min(e);
        if seg_end > seg_start {
            let local_start = seg_start - s;
            let local_end = seg_end - s;
            let Some(content) = original.spans.get(i).map(|s| s.content.as_ref()) else {
                continue;
            };
            let Some(slice) = content.get(local_start..local_end) else {
                continue;
            };
            acc.push(Span {
                style: *style,
                content: Cow::Borrowed(slice),
            });
        }
        if e >= end_byte {
            break;
        }
    }
    Line {
        style: original.style,
        alignment: original.alignment,
        spans: acc,
    }
}

/// Word-wrap a header line with hanging indent.
///
/// The first span stays on line 1; remaining content wraps with `extra_indent + prefix_width` hanging indent on continuation lines.
pub fn wrap_header_hanging(
    header: Line<'static>,
    width: usize,
    extra_indent: usize,
) -> Vec<Line<'static>> {
    if header.spans.is_empty() {
        return vec![header];
    }

    let Some(prefix_span) = header.spans.first().cloned() else {
        return vec![header];
    };
    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix_span.content.as_ref());
    let total_indent = extra_indent + prefix_width;
    let wrap_width = width.saturating_sub(total_indent);

    if wrap_width == 0 || header.spans.len() < 2 {
        return word_wrap_lines(std::iter::once(header), width);
    }

    let Some(rest) = header.spans.get(1..) else {
        return word_wrap_lines(std::iter::once(header), width);
    };
    let content_line = Line::from(rest.to_vec());
    let mut wrapped = word_wrap_lines(std::iter::once(content_line), wrap_width);

    if let Some(first) = wrapped.first_mut() {
        first.spans.insert(0, prefix_span);
    }
    if total_indent > 0 {
        let indent_str: String = " ".repeat(total_indent);
        for line in wrapped.iter_mut().skip(1) {
            line.spans
                .insert(0, ratatui::text::Span::raw(indent_str.clone()));
        }
    }

    wrapped
}

/// Continuation lines indent to a fixed column (usually the bullet width), not under the content after the prefix.
pub fn wrap_header_flush(header: Line<'static>, width: usize, indent: usize) -> Vec<Line<'static>> {
    let wrap_width = width.saturating_sub(indent);
    let mut wrapped = word_wrap_lines(std::iter::once(header), wrap_width);
    if indent > 0 {
        let indent_str: String = " ".repeat(indent);
        for line in wrapped.iter_mut().skip(1) {
            line.spans
                .insert(0, ratatui::text::Span::raw(indent_str.clone()));
        }
    }
    wrapped
}
