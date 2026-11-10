//! Adapted from Grok Build 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8,
//! crates/codegen/xai-grok-pager/src/app/agent_view/selection.rs and app/mouse.rs.
//! Copyright 2023-2026 SpaceXAI. SPDX-License-Identifier: Apache-2.0.
//! Dot modifications: single-pane state, immutable async layout snapshots, shared
//! clipboard provider and bounded timer. No Grok agents, config, tips or telemetry.
use crate::scrollback::{
    table_geometry::{CellRef, TableGeometry},
    text_selection::*,
    types::*,
};
use crate::{App, ui};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use std::time::{Duration, Instant};
use xai_ratatui_textarea::ClipboardProvider;

const FLASH_DURATION: Duration = Duration::from_millis(150);
// Dot's main loop has no upstream animation ticker; cap this one at 20 Hz.
const AUTOSCROLL_TICK: Duration = Duration::from_millis(50);
const MULTI_CLICK_TIMEOUT_MS: u128 = 300;
const CLIPBOARD_TOAST_DEBOUNCE_MS: u128 = 500;

#[derive(Clone, Copy)]
struct TextClickState {
    time: Instant,
    hit: RangeHit,
    click_count: u8,
}

#[derive(Default)]
pub(crate) struct HistorySelection {
    model: ResolvedSelectionModel,
    snapshot: usize,
    pending: Option<PendingTextDrag>,
    pub(crate) active: Option<ActiveTextDrag>,
    persistent: Option<PersistentTextSelection>,
    pending_block: Option<PendingBlockDrag>,
    block: Option<ActiveBlockDrag>,
    deferred: Option<(u16, u16)>,
    table: Option<TableGeometry>,
    source: Vec<BlockLine>,
    blocks: Vec<String>,
    pointer: Option<(u16, u16)>,
    autoscroll: Option<DragAutoScrollState>,
    last_tick: Option<Instant>,
    created: Option<Instant>,
    last_text_click: Option<TextClickState>,
    last_clipboard_toast_at: Option<Instant>,
}
impl HistorySelection {
    pub(crate) fn clear(&mut self) {
        self.pending = None;
        self.active = None;
        self.persistent = None;
        self.pending_block = None;
        self.block = None;
        self.deferred = None;
        self.table = None;
        self.source.clear();
        self.blocks.clear();
        self.pointer = None;
        self.autoscroll = None;
        self.last_tick = None;
        self.created = None;
        self.last_text_click = None;
    }
    // app/mouse.rs clears the old highlight on Down but keeps multi-click history.
    pub(crate) fn clear_for_press(&mut self) {
        let previous_click = self.last_text_click;
        self.clear();
        self.last_text_click = previous_click;
    }
    pub(crate) fn latched(&self) -> bool {
        self.pending.is_some()
            || self.active.is_some()
            || self.pending_block.is_some()
            || self.block.is_some()
            || self.deferred.is_some()
    }
    fn sync(&mut self, model: ResolvedSelectionModel, snapshot: usize) {
        // A resize/reply/media reflow changes wrapped indices. Never copy from
        // a different snapshot than the frame where the gesture started.
        if self.snapshot != snapshot {
            self.clear();
        }
        self.snapshot = snapshot;
        self.model = model;
        if let (Some(mut drag), Some((col, row))) = (self.active, self.pointer)
            && let Some(head) = self.model.hit_test_nearest_in_range(drag.anchor, col, row)
        {
            drag.head = head;
            drag.kind =
                resolve_table_drag_kind(self.table.as_ref(), &drag.anchor, &head, drag.kind);
            self.active = Some(drag);
        }
    }
    fn text_copy(&self, drag: &ActiveTextDrag) -> Option<(String, SelectionKind)> {
        if drag.kind != SelectionKind::Linear
            && let Some(table) = &self.table
        {
            let source = |i| self.source.get(i).map(derive_selection_text);
            if TableGeometry::detect(source, drag.anchor.block_line_idx).as_ref() == Some(table)
                && let Some(text) = reconstruct_table_selection_text(table, drag, source)
            {
                return Some((text, drag.kind));
            }
        }
        reconstruct_full_selection_text(&self.source, drag)
            .map(|text| (text, SelectionKind::Linear))
    }
    fn finish(&mut self) -> Option<String> {
        let drag = self.active;
        let copied = drag.and_then(|d| self.text_copy(&d));
        let block = self.block;
        self.pending = None;
        self.active = None;
        self.pending_block = None;
        self.block = None;
        self.deferred = None;
        self.autoscroll = None;
        self.pointer = None;
        self.last_tick = None;
        if let Some((text, kind)) = copied
            && !text.is_empty()
        {
            let drag = drag.unwrap();
            self.persistent = Some(PersistentTextSelection {
                entry_idx: drag.anchor.entry_idx,
                range_id: drag.anchor.range_id,
                anchor: SelectionEndpoint {
                    block_line_idx: drag.anchor.block_line_idx,
                    col_within_range: drag.anchor.col_within_range,
                },
                head: SelectionEndpoint {
                    block_line_idx: drag.head.block_line_idx,
                    col_within_range: drag.head.col_within_range,
                },
                origin: SelectionOrigin::Drag,
                kind,
            });
            self.created = Some(Instant::now());
            return Some(text);
        }
        if let Some(block) = block {
            let start = block.anchor_entry_idx.min(block.head_entry_idx);
            let end = block.anchor_entry_idx.max(block.head_entry_idx);
            let text = self
                .blocks
                .iter()
                .enumerate()
                .filter(|(i, t)| *i >= start && *i <= end && !t.is_empty())
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            return (!text.is_empty()).then_some(text);
        }
        None
    }
    fn arm(&mut self, anchor: RangeHit, head: RangeHit, width: Option<u16>, m: MouseEvent) {
        self.last_text_click = None;
        self.table = TableGeometry::detect(
            |i| self.source.get(i).map(derive_selection_text),
            anchor.block_line_idx,
        );
        let kind =
            resolve_table_drag_kind(self.table.as_ref(), &anchor, &head, SelectionKind::Linear);
        self.active = Some(ActiveTextDrag {
            anchor,
            head,
            kind,
            anchor_content_width: width,
        });
        self.pointer = Some((m.column, m.row));
        self.autoscroll = compute_autoscroll(m.row, self.model.content_area);
    }
}
fn refresh(app: &App, area: Rect) {
    let (model, snapshot) = ui::selection_model(app, area);
    app.history_selection.borrow_mut().sync(model, snapshot);
}
fn deliver(app: &mut App, text: Option<String>, show_toast: bool) {
    if let Some(text) = text {
        // A textarea copy may still be pending in this input batch. Preserve it
        // before the next write, then install history feedback immediately as
        // upstream copy_to_clipboard does (before multi-click toast debounce).
        if let Some((outcome, at)) = app.clipboard.take_copy_result() {
            app.copy_feedback.borrow_mut().show(outcome, at);
        }
        app.clipboard.set(&text);
        if let Some((outcome, at)) = app.clipboard.take_copy_result()
            && show_toast
        {
            app.copy_feedback.borrow_mut().show(outcome, at);
        }
    }
}
pub(crate) fn begin(app: &mut App, m: MouseEvent, area: Rect) {
    refresh(app, area);
    let hit = app
        .history_selection
        .borrow()
        .model
        .hit_test_selectable_range(m.column, m.row);
    let source = hit
        .map(|h| ui::selection_source(app, area, h.entry_idx))
        .unwrap_or_default();
    let blocks = ui::selection_blocks(app, area);
    let mut state = app.history_selection.borrow_mut();
    state.clear_for_press();
    if !state
        .model
        .content_area
        .contains(Position::new(m.column, m.row))
    {
        return;
    }
    state.source = source;
    state.blocks = blocks;
    if let Some(anchor) = hit {
        state.pending = Some(PendingTextDrag {
            anchor,
            start_col: m.column,
            start_row: m.row,
            anchor_content_width: state.model.visible_block_content_width(anchor.entry_idx),
        });
    } else {
        state.pending_block = state
            .model
            .hit_test_visible_block(m.column, m.row)
            .filter(|b| b.drag_startable)
            .map(|b| PendingBlockDrag {
                anchor_entry_idx: b.entry_idx,
                start_col: m.column,
                start_row: m.row,
            });
        state.deferred = Some((m.column, m.row));
    }
}
/// Returns true when a drag consumed the gesture (so it cannot fold or edit).
pub(crate) fn motion_or_release(app: &mut App, m: MouseEvent, area: Rect) -> bool {
    refresh(app, area);
    if !app.history_selection.borrow().latched() {
        return false;
    }
    match m.kind {
        MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Moved => {
            // A bare Moved proves the button is up: upstream lost-Up recovery.
            let active = {
                let state = app.history_selection.borrow();
                state.active.is_some() || state.block.is_some()
            };
            let copied = app.history_selection.borrow_mut().finish();
            if active {
                app.history_press = None;
                app.last_history_click = None;
            }
            deliver(app, copied, true);
            active
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let deferred_hit = {
                let state = app.history_selection.borrow();
                state
                    .deferred
                    .and_then(|_| state.model.hit_test_selectable_range(m.column, m.row))
            };
            let source = deferred_hit.map(|h| ui::selection_source(app, area, h.entry_idx));
            let mut state = app.history_selection.borrow_mut();
            if let Some(mut drag) = state.active {
                let head = state
                    .model
                    .hit_test_nearest_in_range(drag.anchor, m.column, m.row)
                    .unwrap_or(drag.head);
                drag.head = head;
                drag.kind =
                    resolve_table_drag_kind(state.table.as_ref(), &drag.anchor, &head, drag.kind);
                state.active = Some(drag);
            } else if let Some(hit) = deferred_hit {
                state.deferred = None;
                state.pending_block = None;
                state.block = None;
                state.source = source.unwrap_or_default();
                let width = state.model.visible_block_content_width(hit.entry_idx);
                state.arm(hit, hit, width, m);
            } else if let Some(pending) = state.pending {
                if !drag_threshold_exceeded(&pending, m.column, m.row) {
                    return true;
                }
                let head = state
                    .model
                    .hit_test_nearest_in_range(pending.anchor, m.column, m.row)
                    .unwrap_or(pending.anchor);
                state.arm(pending.anchor, head, pending.anchor_content_width, m);
            } else {
                if let Some(pending) = state.pending_block
                    && block_drag_threshold_exceeded(&pending, m.column, m.row)
                {
                    state.block = Some(ActiveBlockDrag {
                        anchor_entry_idx: pending.anchor_entry_idx,
                        head_entry_idx: pending.anchor_entry_idx,
                    });
                }
                let head = state
                    .model
                    .hit_test_visible_block(m.column, m.row)
                    .map(|b| b.entry_idx);
                if let (Some(block), Some(head)) = (&mut state.block, head) {
                    block.head_entry_idx = head;
                }
            }
            if state.active.is_some() || state.block.is_some() {
                state.pointer = Some((m.column, m.row));
                state.autoscroll = compute_autoscroll(m.row, state.model.content_area);
                app.history_press = None;
                app.last_history_click = None;
            }
            true
        }
        _ => false,
    }
}
/// Original app/mouse.rs WordSelect precedence and AgentView multi-click routing.
/// Exact body hits select text; blank padding falls through to ordinary fold clicks.
pub(crate) fn text_click(app: &mut App, col: u16, row: u16, area: Rect) -> bool {
    if !app.text_selection.selects_word() {
        return false;
    }
    refresh(app, area);
    let hit = app
        .history_selection
        .borrow()
        .model
        .hit_test_text_exact(col, row);
    let Some(hit) = hit else {
        app.history_selection.borrow_mut().last_text_click = None;
        return false;
    };
    let source = ui::selection_source(app, area, hit.entry_idx);
    let now = Instant::now();
    let (handled, copied, suppress_toast) = {
        let mut state = app.history_selection.borrow_mut();
        state.source = source;
        let count = if let Some(previous) = state.last_text_click
            && previous.hit.entry_idx == hit.entry_idx
            && previous.hit.range_id == hit.range_id
            && previous.hit.block_line_idx == hit.block_line_idx
            && now.duration_since(previous.time).as_millis() < MULTI_CLICK_TIMEOUT_MS
        {
            previous.click_count.saturating_add(1)
        } else {
            1
        };
        let copied = match count {
            1 => None,
            2 => state.select_word_at(&hit),
            3 => state
                .select_cell_at(&hit)
                .or_else(|| state.select_paragraph_at(&hit)),
            _ => {
                state.last_text_click = None;
                return false;
            }
        };
        state.last_text_click = Some(TextClickState {
            time: now,
            hit,
            click_count: count,
        });
        let suppress = copied.is_some()
            && state
                .last_clipboard_toast_at
                .is_some_and(|t| now.duration_since(t).as_millis() < CLIPBOARD_TOAST_DEBOUNCE_MS);
        if copied.is_some() && !suppress {
            state.last_clipboard_toast_at = Some(now);
        }
        (true, copied, suppress)
    };
    deliver(app, copied.filter(|text| !text.is_empty()), !suppress_toast);
    if handled {
        app.last_history_click = None;
    }
    handled
}
impl HistorySelection {
    // Original select_word_at: semantic_selection_at handles wrapped words/URLs.
    fn select_word_at(&mut self, hit: &RangeHit) -> Option<String> {
        let selection = semantic_selection_at(&self.model, hit, configured_word_separators())?;
        self.persistent = Some(PersistentTextSelection {
            entry_idx: hit.entry_idx,
            range_id: hit.range_id,
            anchor: selection.anchor,
            head: selection.head,
            origin: SelectionOrigin::DoubleClick,
            kind: SelectionKind::Linear,
        });
        self.created = Some(Instant::now());
        Some(selection.text)
    }
    fn select_line_at(&mut self, hit: &RangeHit) -> Option<String> {
        let line = self.model.line_for_hit(hit)?;
        let width = line
            .selectable_cols
            .end
            .saturating_sub(line.selectable_cols.start);
        if width == 0 {
            return None;
        }
        let text = line.text.clone();
        self.persistent = Some(PersistentTextSelection {
            entry_idx: hit.entry_idx,
            range_id: hit.range_id,
            anchor: SelectionEndpoint {
                block_line_idx: hit.block_line_idx,
                col_within_range: 0,
            },
            head: SelectionEndpoint {
                block_line_idx: hit.block_line_idx,
                col_within_range: width.saturating_sub(1),
            },
            origin: SelectionOrigin::TripleClick,
            kind: SelectionKind::Linear,
        });
        self.created = Some(Instant::now());
        Some(text)
    }
    // Original select_paragraph_at logical-line walk; never absorbs a neighboring
    // paragraph/list item merely because it is visible in the same message.
    fn select_paragraph_at(&mut self, hit: &RangeHit) -> Option<String> {
        let resolved = {
            let range = self.model.range(hit.entry_idx, hit.range_id)?;
            let click_pos = range
                .lines
                .iter()
                .position(|line| line.block_line_idx == hit.block_line_idx)?;
            let continues = |a: usize, b: usize| match (range.lines.get(a), range.lines.get(b)) {
                (Some(a), Some(b)) => {
                    a.block_line_idx + 1 == b.block_line_idx && b.joiner_to_previous.is_some()
                }
                _ => false,
            };
            let mut start = click_pos;
            while start > 0 && continues(start - 1, start) {
                start -= 1;
            }
            let mut end = click_pos;
            while continues(end, end + 1) {
                end += 1;
            }
            match (range.lines.get(start), range.lines.get(end)) {
                (Some(first), Some(last)) => Some((
                    first.block_line_idx,
                    last.block_line_idx,
                    last.selectable_cols
                        .end
                        .saturating_sub(last.selectable_cols.start),
                )),
                _ => None,
            }
        };
        let Some((first, last, width)) = resolved else {
            return self.select_line_at(hit);
        };
        if first == last {
            return self.select_line_at(hit);
        }
        if width == 0 {
            return None;
        }
        let drag = ActiveTextDrag {
            anchor: RangeHit {
                entry_idx: hit.entry_idx,
                range_id: hit.range_id,
                block_line_idx: first,
                col_within_range: 0,
            },
            head: RangeHit {
                entry_idx: hit.entry_idx,
                range_id: hit.range_id,
                block_line_idx: last,
                col_within_range: width.saturating_sub(1),
            },
            kind: SelectionKind::Linear,
            anchor_content_width: None,
        };
        let copied = reconstruct_selection_text(&self.model, &drag);
        self.persistent = Some(PersistentTextSelection {
            entry_idx: hit.entry_idx,
            range_id: hit.range_id,
            anchor: SelectionEndpoint {
                block_line_idx: first,
                col_within_range: 0,
            },
            head: SelectionEndpoint {
                block_line_idx: last,
                col_within_range: width.saturating_sub(1),
            },
            origin: SelectionOrigin::TripleClick,
            kind: SelectionKind::Linear,
        });
        self.created = Some(Instant::now());
        copied
    }
    // Original select_cell_at / select_whole_table_at, using the imported table
    // geometry. Dot has no rich-source TableCopyMeta (documented port boundary).
    fn select_cell_at(&mut self, hit: &RangeHit) -> Option<String> {
        let geometry = TableGeometry::detect(
            |i| self.source.get(i).map(derive_selection_text),
            hit.block_line_idx,
        )?;
        let (anchor, head, kind, text) =
            if let Some(cell) = geometry.cell_at(hit.block_line_idx, hit.col_within_range) {
                let lines = geometry.row_lines(cell.row);
                let band = geometry.band(cell.col);
                (
                    SelectionEndpoint {
                        block_line_idx: lines.start,
                        col_within_range: band.start,
                    },
                    SelectionEndpoint {
                        block_line_idx: lines.end.saturating_sub(1),
                        col_within_range: band.end.saturating_sub(1),
                    },
                    SelectionKind::TableCell,
                    geometry.cell_text(cell, |i| self.source.get(i).map(derive_selection_text)),
                )
            } else {
                let anchor = CellRef { row: 0, col: 0 };
                let head = CellRef {
                    row: geometry.n_rows() - 1,
                    col: geometry.n_cols() - 1,
                };
                let first = geometry.row_lines(0);
                let last = geometry.row_lines(head.row);
                (
                    SelectionEndpoint {
                        block_line_idx: first.start,
                        col_within_range: geometry.band(0).start,
                    },
                    SelectionEndpoint {
                        block_line_idx: last.end.saturating_sub(1),
                        col_within_range: geometry.band(head.col).end.saturating_sub(1),
                    },
                    SelectionKind::TableGrid { anchor, head },
                    geometry.grid_tsv(anchor, head, |i| {
                        self.source.get(i).map(derive_selection_text)
                    }),
                )
            };
        self.persistent = Some(PersistentTextSelection {
            entry_idx: hit.entry_idx,
            range_id: hit.range_id,
            anchor,
            head,
            origin: SelectionOrigin::TripleClick,
            kind,
        });
        self.table = Some(geometry);
        self.created = Some(Instant::now());
        Some(text)
    }
}
pub(crate) fn render(app: &App, model: ResolvedSelectionModel, snapshot: usize, buf: &mut Buffer) {
    // Paint against the exact immutable snapshot used by Paragraph this frame.
    app.history_selection.borrow_mut().sync(model, snapshot);
    let mut state = app.history_selection.borrow_mut();
    if !app.text_selection.holds() && state.created.is_some_and(|t| t.elapsed() >= FLASH_DURATION) {
        state.persistent = None;
        state.created = None;
        if state.active.is_none() {
            state.table = None;
        }
    }
    if let Some(drag) = &state.active {
        render_active_selection_overlay(&state.model, drag, state.table.as_ref(), buf);
    }
    if let Some(selection) = &state.persistent {
        render_persistent_selection_overlay(&state.model, selection, state.table.as_ref(), buf);
    }
    if let Some(block) = &state.block {
        render_block_drag_overlay(&state.model, block, buf);
    }
}
pub(crate) fn tick(app: &mut App, area: Rect, now: Instant) {
    refresh(app, area);
    let scroll = {
        let mut state = app.history_selection.borrow_mut();
        let Some(scroll) = state.autoscroll else {
            return;
        };
        if state
            .last_tick
            .is_some_and(|t| now.duration_since(t) < AUTOSCROLL_TICK)
        {
            return;
        }
        state.last_tick = Some(now);
        scroll
    };
    // Keep the captured wrapped snapshot; wheel/key navigation instead clears it.
    app.follow_latest = false;
    app.fold_anchor = None;
    app.reading_anchor = None;
    let limit = ui::timeline(app, area).limit();
    let delta = match scroll.direction {
        AutoScrollDirection::Up => -(scroll.speed as isize),
        AutoScrollDirection::Down => scroll.speed as isize,
    };
    app.scroll = app
        .scroll
        .min(limit)
        .saturating_add_signed(delta)
        .min(limit);
    refresh(app, area);
}
pub(crate) fn wait(app: &App) -> Option<Duration> {
    let state = app.history_selection.borrow();
    if state.autoscroll.is_some() {
        Some(AUTOSCROLL_TICK)
    } else if app.text_selection.holds() {
        None
    } else {
        state
            .created
            .map(|t| FLASH_DURATION.saturating_sub(t.elapsed()))
    }
}
pub(crate) fn copy_key(app: &mut App) -> bool {
    let copied = {
        let state = app.history_selection.borrow();
        if let Some(active) = &state.active {
            state.text_copy(active).map(|(t, _)| t)
        } else if let Some(p) = state.persistent {
            state
                .text_copy(&ActiveTextDrag {
                    anchor: RangeHit {
                        entry_idx: p.entry_idx,
                        range_id: p.range_id,
                        block_line_idx: p.anchor.block_line_idx,
                        col_within_range: p.anchor.col_within_range,
                    },
                    head: RangeHit {
                        entry_idx: p.entry_idx,
                        range_id: p.range_id,
                        block_line_idx: p.head.block_line_idx,
                        col_within_range: p.head.col_within_range,
                    },
                    kind: p.kind,
                    anchor_content_width: None,
                })
                .map(|(t, _)| t)
        } else {
            None
        }
    };
    let copied_any = copied.is_some();
    deliver(app, copied, true);
    copied_any
}
