//! PromptWidget Enter router, extracted from Grok Build fixed 2bdd1d6,
//! pager/src/views/prompt_widget/mod.rs; xAI, Apache-2.0.
//! Dot has no file-search dropdown. The textarea and insertion operations are
//! passed directly; the original Enter routing order and continuation logic
//! remain. Bare Enter submits independently of keyboard protocol negotiation.
//! Modified Enter inserts a newline; Apple Terminal modifier rescue is sampled
//! when the host reads the event, before it queues/coalesces input.
use crossterm::event::{KeyCode, KeyEvent};
use xai_ratatui_textarea::TextArea;
#[derive(Debug, PartialEq, Eq)]
pub enum EnterOutcome {
    NewlineInserted,
    Submit,
    PassThrough,
}
struct PromptWidget<'a> {
    textarea: &'a mut TextArea,
}
impl PromptWidget<'_> {
    fn file_search_visible(&self) -> bool {
        false
    }
    fn insert_replacing_selection(&mut self, text: &str) {
        self.textarea.insert_str_replacing_selection(text);
    }
    fn apply_backslash_continuation(&mut self) -> bool {
        let cursor = self.textarea.cursor();
        if cursor > 0 && self.textarea.text().as_bytes().get(cursor - 1) == Some(&b'\\') {
            self.textarea.delete_backward(1);
            self.textarea.insert_str("\n");
            return true;
        }
        false
    }
    // Original function body, including file-search, Apple modifier rescue,
    // backslash continuation, bare-submit and modified-Enter fallthrough.
    fn route_enter_with_newline_modifier(
        &mut self,
        key: &KeyEvent,
        is_newline_modifier_held: bool,
    ) -> EnterOutcome {
        if self.file_search_visible() {
            return EnterOutcome::PassThrough;
        }
        if key.code == KeyCode::Enter && is_newline_modifier_held {
            self.insert_replacing_selection("\n");
            return EnterOutcome::NewlineInserted;
        }
        if key.code == KeyCode::Enter
            && key.modifiers.is_empty()
            && self.apply_backslash_continuation()
        {
            return EnterOutcome::NewlineInserted;
        }
        if key.code == KeyCode::Enter && key.modifiers.is_empty() {
            return EnterOutcome::Submit;
        }
        EnterOutcome::PassThrough
    }
}
pub fn route(textarea: &mut TextArea, key: &KeyEvent, newline_modifier_held: bool) -> EnterOutcome {
    PromptWidget { textarea }.route_enter_with_newline_modifier(key, newline_modifier_held)
}
