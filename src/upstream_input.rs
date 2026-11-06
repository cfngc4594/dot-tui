// Grok Build, Copyright 2023–2026 SpaceXAI, Apache-2.0.
// Extracted input coalescing from pager/src/app/event_loop.rs at
// 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
// Adaptations: AltGr helper comes from its original textarea crate; Windows
// image-drop routing omitted; split at dot command boundaries so confirmation
// keys cannot be discarded by upstream paste-fragment merging. No Grok backend.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
#[derive(Clone, Debug, PartialEq)]
pub struct TimedInputEvent {
    pub event: Event,
    pub arrived_at: std::time::Instant,
}
impl TimedInputEvent {
    pub fn now(mut event: Event) -> Self {
        rescue_apple_terminal_enter(&mut event);
        Self {
            event,
            arrived_at: std::time::Instant::now(),
        }
    }
}
// Grok input/terminal_support.rs and macos_modifiers.rs (2bdd1d6,
// Apache-2.0): Apple Terminal drops modified-Enter flags. Sample at read time,
// never while replaying an already queued event. This is a best-effort rescue:
// OS modifier state cannot reconstruct a key whose modifier was released before
// the PTY event was read. Remote and multiplexed sessions must not use local flags.
fn rescue_apple_terminal_enter(event: &mut Event) {
    #[cfg(target_os = "macos")]
    if let Event::Key(key) = event
        && key.kind == KeyEventKind::Press
        && key.code == KeyCode::Enter
        && key.modifiers.is_empty()
        && !key
            .state
            .contains(crossterm::event::KeyEventState::CSI_U_ENTER)
    {
        let ctx = crate::terminal::terminal_context();
        if ctx.brand == crate::terminal::TerminalName::AppleTerminal
            && !ctx.is_ssh
            && ctx.graphics_protocol_skip_reason().is_none()
        {
            #[link(name = "CoreGraphics", kind = "framework")]
            unsafe extern "C" {
                fn CGEventSourceFlagsState(state_id: i32) -> u64;
            }
            // Original HID-system-state query and Shift/Option/Command flag bits.
            // No event tap, event injection, or permission change is performed.
            let flags = unsafe { CGEventSourceFlagsState(1) };
            if flags & (0x0002_0000 | 0x0008_0000 | 0x0010_0000) != 0 {
                key.modifiers.insert(KeyModifiers::SHIFT);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = event;
}
const PASTE_COALESCE_THRESHOLD: usize = 3;
fn is_pasteable_key_event(ev: &Event) -> bool {
    // Enter is a command boundary even without CSI-u. Bracketed paste remains
    // Event::Paste, so paste contents never become submit key events.
    if matches!(ev, Event::Key(k) if k.code == KeyCode::Enter) {
        return false;
    }
    match ev {
        Event::Key(ke) if ke.kind == KeyEventKind::Press => match ke.code {
            KeyCode::Char(_) => {
                ke.modifiers.is_empty()
                    || ke.modifiers == KeyModifiers::SHIFT
                    || xai_ratatui_textarea::is_altgr(ke.modifiers)
            }
            KeyCode::Enter | KeyCode::Tab => ke.modifiers.is_empty(),
            _ => false,
        },
        _ => false,
    }
}
fn is_paste_lf(ev: &Event) -> bool {
    matches!(ev, Event::Key(ke)
        if ke.kind == KeyEventKind::Press
            && ke.code == KeyCode::Char('j')
            && ke.modifiers == KeyModifiers::CONTROL)
}
fn is_voice_chord(ke: &KeyEvent) -> bool {
    match ke.kind {
        KeyEventKind::Release => matches!(ke.code, KeyCode::Char(' ') | KeyCode::F(8)),
        _ => {
            (ke.code == KeyCode::Char(' ') && ke.modifiers == KeyModifiers::CONTROL)
                || (ke.code == KeyCode::F(8) && ke.modifiers.is_empty())
        }
    }
}
fn coalesce_live_keys(events: Vec<TimedInputEvent>) -> Vec<TimedInputEvent> {
    if events.len() < PASTE_COALESCE_THRESHOLD {
        return events;
    }
    let (mut has_paste, mut has_keys) = (false, false);
    for e in &events {
        has_paste |= matches!(e.event, Event::Paste(_));
        has_keys |= is_pasteable_key_event(&e.event);
    }
    if has_paste {
        return if has_keys {
            merge_paste_fragments(events)
        } else {
            events
        };
    }
    let events: Vec<TimedInputEvent> = events
        .into_iter()
        .filter(|ev| {
            !matches!(&ev.event, Event::Key(ke)
                if ke.kind == KeyEventKind::Release && !is_voice_chord(ke))
        })
        .collect();
    let mut result = Vec::with_capacity(events.len());
    let mut i = 0;
    while i < events.len() {
        let Some(ev) = events.get(i) else {
            break;
        };
        if is_pasteable_key_event(&ev.event) {
            let run_start = i;
            let arrived_at = ev.arrived_at;
            let mut text = String::new();
            let mut seen_enter = false;
            let mut has_char_after_enter = false;
            let mut has_crlf = false;
            let mut prev_was_enter = false;
            while i < events.len() {
                let Some(ev) = events.get(i) else {
                    break;
                };
                if is_pasteable_key_event(&ev.event) {
                    if let Event::Key(ke) = &ev.event {
                        match ke.code {
                            KeyCode::Char(c) => {
                                text.push(c);
                                if seen_enter {
                                    has_char_after_enter = true;
                                }
                                prev_was_enter = false;
                            }
                            KeyCode::Enter => {
                                text.push('\n');
                                seen_enter = true;
                                prev_was_enter = true;
                            }
                            KeyCode::Tab => {
                                text.push('\t');
                                if seen_enter {
                                    has_char_after_enter = true;
                                }
                                prev_was_enter = false;
                            }
                            _ => unreachable!("is_pasteable_key_event guards this"),
                        }
                    }
                    i += 1;
                } else if prev_was_enter && is_paste_lf(&ev.event) {
                    has_crlf = true;
                    prev_was_enter = false;
                    i += 1;
                } else {
                    break;
                }
            }
            let run_len = i - run_start;
            let multiline_paste =
                (run_len >= PASTE_COALESCE_THRESHOLD && has_char_after_enter) || has_crlf;
            let path_shaped_drop = false;
            if multiline_paste || path_shaped_drop {
                tracing::debug!(
                    run_len,
                    text_len = text.len(),
                    path_shape = path_shaped_drop,
                    "coalesced rapid key events into paste"
                );
                result.push(TimedInputEvent {
                    event: Event::Paste(text),
                    arrived_at,
                });
            } else if let Some(run) = events.get(run_start..i) {
                for ev in run {
                    result.push(ev.clone());
                }
            }
        } else {
            result.push(ev.clone());
            i += 1;
        }
    }
    result
}
fn merge_paste_fragments(events: Vec<TimedInputEvent>) -> Vec<TimedInputEvent> {
    let mut result = Vec::new();
    let mut merged_text = String::new();
    let mut merged_arrived_at = None;
    for ev in events {
        match &ev.event {
            Event::Paste(text) => {
                merged_arrived_at.get_or_insert(ev.arrived_at);
                merged_text.push_str(text);
            }
            Event::Key(ke) if is_pasteable_key_event(&ev.event) => {
                merged_arrived_at.get_or_insert(ev.arrived_at);
                match ke.code {
                    KeyCode::Char(c) => merged_text.push(c),
                    KeyCode::Enter => merged_text.push('\n'),
                    KeyCode::Tab => merged_text.push('\t'),
                    _ => {}
                }
            }
            Event::Key(_) => {}
            _ => {
                if !merged_text.is_empty() {
                    result.push(TimedInputEvent {
                        event: Event::Paste(std::mem::take(&mut merged_text)),
                        arrived_at: merged_arrived_at
                            .take()
                            .expect("non-empty merged paste has an arrival time"),
                    });
                }
                result.push(ev);
            }
        }
    }
    if !merged_text.is_empty() {
        result.push(TimedInputEvent {
            event: Event::Paste(merged_text),
            arrived_at: merged_arrived_at.expect("non-empty merged paste has an arrival time"),
        });
    }
    result
}
/// The original coalescer is used only inside ordered text runs. Dot's modal
/// commands are retained as boundaries, not passed to paste fragment merging.
pub fn normalize_batch(events: Vec<TimedInputEvent>) -> Vec<TimedInputEvent> {
    let mut out = Vec::new();
    let mut text = Vec::new();
    for event in events {
        if is_pasteable_key_event(&event.event)
            || is_paste_lf(&event.event)
            || matches!(event.event, Event::Paste(_))
        {
            text.push(event);
        } else {
            out.extend(coalesce_live_keys(std::mem::take(&mut text)));
            out.push(event);
        }
    }
    out.extend(coalesce_live_keys(text));
    out
}
