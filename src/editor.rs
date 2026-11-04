//! Small grapheme-aware composer. Rendering and caret positioning share one
//! display-cell layout, including explicit newlines and soft wraps.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Draft {
    pub text: String,
    pub cursor: usize,
}
impl Draft {
    pub fn insert(&mut self, text: &str) {
        self.cursor = boundary(&self.text, self.cursor);
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        // Insertion can join adjacent graphemes (combining marks/ZWJ).
        while self.cursor < self.text.len()
            && !self
                .text
                .grapheme_indices(true)
                .any(|(i, _)| i == self.cursor)
        {
            self.cursor += self.text[self.cursor..].chars().next().unwrap().len_utf8();
        }
    }
    pub fn left(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|&i| i < self.cursor)
            .last()
            .unwrap_or(0);
    }
    pub fn right(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|&i| i > self.cursor)
            .unwrap_or(self.text.len());
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
    }
    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
    }
    pub fn vertical(&mut self, width: usize, down: bool) {
        let view = layout(&self.text, self.cursor, width, usize::MAX);
        let target = if down {
            view.cursor.0.saturating_add(1)
        } else {
            view.cursor.0.saturating_sub(1)
        };
        if let Some((i, _, _)) = positions(&self.text, width)
            .into_iter()
            .filter(|(_, y, _)| *y == target)
            .min_by_key(|(i, _, x)| (x.abs_diff(view.cursor.1), std::cmp::Reverse(*i)))
        {
            self.cursor = i;
        }
    }
}
fn boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|&i| i <= cursor)
        .last()
        .unwrap_or(0)
}

pub struct View {
    pub rows: Vec<String>,
    pub cursor: (usize, usize), // visual row, display cell column
    pub offset: usize,
}
fn wrap(text: &str, width: usize) -> (Vec<String>, Vec<(usize, usize, usize)>) {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut x = 0;
    let mut points = Vec::new();
    for (i, g) in text.grapheme_indices(true) {
        let n = g.width().min(width);
        if g != "\n" && x > 0 && x + n > width {
            rows.push(std::mem::take(&mut row));
            x = 0;
        }
        let caret = if x == width {
            (rows.len() + 1, 0)
        } else {
            (rows.len(), x)
        };
        points.push((i, caret.0, caret.1));
        if g == "\n" {
            rows.push(std::mem::take(&mut row));
            x = 0;
        } else {
            row.push_str(if g.width() > width { "�" } else { g });
            x += n;
        }
    }
    if x == width {
        rows.push(std::mem::take(&mut row));
        x = 0;
    }
    points.push((text.len(), rows.len(), x));
    rows.push(row);
    (rows, points)
}
fn positions(text: &str, width: usize) -> Vec<(usize, usize, usize)> {
    wrap(text, width).1
}
pub fn layout(text: &str, cursor: usize, width: usize, height: usize) -> View {
    let (rows, points) = wrap(text, width);
    let cursor = boundary(text, cursor);
    let (_, y, x) = points.into_iter().find(|(i, _, _)| *i == cursor).unwrap();
    View {
        rows,
        cursor: (y, x),
        offset: y.saturating_sub(height.max(1) - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_wrap_and_explicit_newline_share_a_caret_row() {
        let v = layout("中文", "中文".len(), 4, 2);
        assert_eq!(v.rows, vec!["中文", ""]);
        assert_eq!(v.cursor, (1, 0));
        let v = layout("中文\na", "中文\n".len(), 4, 2);
        assert_eq!(v.rows, vec!["中文", "a"]);
        assert_eq!(v.cursor, (1, 0));
    }
    #[test]
    fn combining_and_emoji_edits_never_split_a_grapheme() {
        let mut d = Draft::default();
        d.insert("e");
        d.insert("\u{301}");
        d.insert("👩‍💻");
        d.left();
        assert_eq!(d.cursor, "e\u{301}".len());
        d.backspace();
        assert_eq!(d.text, "👩‍💻");
        assert_eq!(d.cursor, 0);
        d.delete();
        assert!(d.text.is_empty());
        d.insert("中\n文");
        d.home();
        assert_eq!(d.cursor, "中\n".len());
        d.left();
        d.backspace();
        assert_eq!(d.text, "\n文");
    }
    #[test]
    fn every_caret_boundary_stays_visible_in_long_wrapped_text() {
        let text = "e\u{301}中👩‍💻 and words\n\n".repeat(8);
        for width in [1, 2, 5, 32] {
            for cursor in text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
            {
                let v = layout(&text, cursor, width, 3);
                assert!(v.cursor.1 < width);
                assert!(v.cursor.0 >= v.offset && v.cursor.0 < v.offset + 3);
                assert!(v.rows.iter().all(|s| s.width() <= width));
            }
        }
    }
    #[test]
    fn blank_start_and_end_deletions_are_noops() {
        let mut d = Draft::default();
        d.left();
        d.right();
        d.delete();
        d.backspace();
        d.home();
        d.end();
        assert_eq!(d.cursor, 0);
        assert!(d.text.is_empty());
    }
}
