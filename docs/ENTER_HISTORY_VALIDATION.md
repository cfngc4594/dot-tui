# Direct Enter, history anchors, focus, user folding and outer scrollbar

This is incremental on dot-tui-footer-key-action-style (previous binary
bf972208b905cd8d6674dd0fc0f35b4a1c4ca3cfd3a1f31258bb4cebb857f77b).
No GitHub mutation was performed. Full updated source and an incremental patch
are included; the patch was applied to the captured pre-change source and the
resulting changed bytes were checked against the actual Mac source.

## Enter submission

The per-message review button, modal, Y confirmation and Ctrl+S review route
were removed. Enter now explicitly submits, with original PromptWidget routing
order: file-search (absent in dot), Apple newline modifier flag, backslash
continuation, bare Enter submit, then modified Enter fallthrough to the original
textarea input dispatcher. The route and continuation bodies are extracted in
upstream_enter.rs; the host supplies false for the CoreGraphics modifier query.
Alt+Enter was verified with actual escape-prefixed Enter through the PTY.
Shift+Enter is supported when the terminal actually reports Shift. No global
keyboard permissions or Apple Terminal window access were requested.
Crossterm has no IME preedit/candidate state: the terminal must consume candidate
acceptance before emitting committed keys; an indistinguishable bare Enter
cannot be classified as Shift or IME acceptance. This is an explicit limit.

The transport is unchanged: one dot, successful room/history binding, nonempty
message, enabled and visible prompt, not busy/pending/uncertain, and (for live
mode) a journal are all required. Every explicit attempt has one UUID reused as
request_id/idempotency_token by the existing backend. The journal is persisted
before Job::Send. Unknown results remain blocked across restart; refresh cannot
unlock them, and there is no automatic network retry. Accepted sends clear the
draft; errors preserve it. Repeat/release Enter is ignored. Unchanged drafts
cannot be retried after rejection without editing. A paste/focus transition
blocks submission for its whole collected input batch, plus a 150ms post-paste
guard. Bracketed-paste newlines are text.

## History

Latest initial history follows the bottom. A further upward wheel event at
scroll zero requests one older page; busy/layout and exhausted/failure guards
prevent request storms. Ctrl+H remains an explicit equivalent and retry route.
The original Grok wheel normalizer and interactive scrollbar are retained.
Replies deduplicate messages and preserve the visible message ID plus its
intra-message visual-row ordinal. Restoration waits for the matching background
layout, including after resize; after reflow the ordinal is clamped to that
message's new height. This is a message/visual-row anchor, not a pixel or byte
position guarantee. User scrolling during pending same-geometry layout updates
the anchor. Empty/duplicate pages stop; errors pause automatic older loading;
288-message limit has distinct wording and does not claim server exhaustion.
The dev history fixture now really pages 32 messages instead of bypassing paging
for timing experiments. Earlier performance logs remain historical evidence.

## Focus and styles

Source: app/mouse.rs, app/agent_view/{panes,input}.rs, PromptWidget draw rules.
Clicking the composer focuses Prompt and uses the original textarea mouse
handler. Clicking elsewhere switches to Scrollback; rail and footer actions do
not focus input. Space/Tab in Scrollback focuses Prompt and is consumed; Space
in Prompt is ordinary input. Unfocused Enter cannot send. Focus changes create
a paint boundary and suppress same-batch submission. Modal/narrow/resize guards
remain. The focused empty placeholder is hidden; unfocused empty placeholder is
shown, caret hidden, and content uses the original 0.66 recede blend.
The shortcut bar swaps Enter/Alt+Enter for Space:输入, keeping original key/action
styles and five-column separators, with no review-button width reservation.

## User-message folds

Original blocks/user.rs policy: COLLAPSED_MAX_LINES=3; foldability estimates
Unicode display widths against conservative 60-column content and folds only
when estimated visual rows exceed three. The original default/next-mode bodies
are retained. Dot's plain text wrapper adapts the original truncation/rewrap
branch, including final-row ` …`; skill/image/cron/bash and agent/tool blocks
are not dot data and are omitted. Only role `you` is folded; assistant/unknown
messages retain their full existing rendering. State is keyed by message ID and
survives refresh. Original same-entry 300ms click counting is retained; the fold
route does not impose an additional coordinate tolerance. The separate original
one-row/column drag threshold cancels a pending click; release must still hit
the same entry. A third click does not toggle a user prompt. Like upstream,
user toggle anchors the clicked entry's top, with layout applied asynchronously.

The upstream standalone TextSelection enum is retained unchanged, including
WordSelect precedence. Dot uses upstream's compile-time Flash default and does
not read remote Grok settings. WordSelect is explicitly tested to bypass folds;
its word/paragraph selection UI and scrollback selection/copy are not exposed
by this adapter. Drag gestures are never misinterpreted as fold clicks. This is
not a claim to have ported the entire Grok selection framework.

## Outer scrollbar

AgentViewLayout uses scrollbar_x=outer_area.right()-(gap_right+1).
Actual ScrollbarConfig defaults are enabled=true, gap_left=0/gap_right=0.
Content_end_x is calculated separately and clamped to the padded scrollback
right edge. Thus at width 100, track x=99 while content remains x=2..98,
rather than placing the track inside the inner body at x=97. Painting, thumb
inversion and grab-zone hit testing share the resulting track rectangle.
Background layout no longer unnecessarily rewraps two columns narrower when
the outer padding already accommodates the scrollbar. Tests include offset Rects.

## Validation and limits

Default, dev-demo and release suites: 137 passed each, zero failures. fmt and
strict all-targets clippy passed in both feature configurations. Actual final
release build and PTY evidence are included separately in logs/validation.json.
The first PTY harness used an outdated acceptance-text marker; corrected to the
current visible caption. The next PTY run passed sending/focus/folding/paging
but exposed an offline-only status override hiding 'no more'; that override
was removed and the final run was repeated. No real credentials or messages were
used. PTY examples are entirely synthetic. The terminal state is restored on exit.

Four private user-provided screenshots could not be materialized through the
supported file-transfer helper. No image bytes were downloaded or viewed in
that validation. Reported observations and the fixed upstream source informed
the work. No refused terminal-window access was retried or bypassed, and no
same-font, live-window or pixel-identity verification is claimed.
