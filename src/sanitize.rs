// Adapted from Grok Build, Copyright 2023-2026 SpaceXAI, Apache-2.0.
// Source: crates/codegen/xai-grok-pager/src/views/goal_detail.rs
// Commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8
// Changes: additionally replace bidi formatting controls; bound rendered text.
pub fn strip_control_chars(s: &str, keep_newlines: bool) -> String {
    s.chars().take(100_000).map(|c| {
        if (c.is_control() && !(keep_newlines && c == '\n'))
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            ' '
        } else { c }
    }).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_newlines() {
        assert_eq!(strip_control_chars("中文\n🙂", true), "中文\n🙂");
    }
    #[test]
    fn controls() {
        let s = strip_control_chars("a\x1b]52;c;X\x07\u{202e}b", false);
        assert!(!s.chars().any(char::is_control));
        assert!(!s.contains('\u{202e}'));
    }
}
