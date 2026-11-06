# Focus, clipboard and background-history audit repair

Incremental on dot-tui-shift-enter-safety-fix (old production SHA256
841a3d9e89c3484e9c634b3fe3efd794ece3154521ad3bbe981b83f4fff7d6e2).
No GitHub mutation or real account/clipboard access was performed.

## Fixed upstream evidence and necessary dot adaptation

All reference source is fixed Grok Build commit
2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8; relevant original files are included.

1. app/event_loop.rs handles FocusGained/FocusLost separately, reasserts mouse
   capture and delegates prompt restoration to AgentView. agent_view/input.rs
   should_restore_prompt_on_focus_gained checks Scrollback/no modal and returns
   non-vim idle when no blocking overlay exists. Dot has one text pane, no vim or
   Grok overlays; idle is mapped to its local network/send operation state.
   EnableFocusChange/DisableFocusChange are paired on normal and panic cleanup.
   FocusLost cancels scroll/drag/click latches and blocks inactive keys/mouse/paste;
   FocusGained reasserts capture, conditionally restores Prompt, and creates the
   existing same-batch submission barrier. It never enables bottom-follow.
   No image or clipboard prewarm is performed on focus events.

   PageUp/PageDown/Home/End route to history only when Scrollback owns input.
   Unmodified page keys move by the actual viewport height minus one. Modified
   page keys remain with textarea, and plain Prompt keys use the original
   textarea route (the widget does not implement PageUp/PageDown movement).
   Upstream also offers an opt-in prompt_paging path for conversation paging;
   dot intentionally uses the non-forced path, with no dropdown/history panel.
   This scope does not claim all Grok pane registries were transplanted.

2. PromptWidget installs SystemClipboard through the original ClipboardProvider.
   The widget's actual native copy arm is Cmd/Super+C; agent-level Ctrl+C has
   CancelTurn semantics and only certain minimal/idle paths quit. Dot's former
   unconditional Ctrl+C exit was wrong. Dot now aliases selected Ctrl+C to the
   original Cmd+C textarea copy arm; without a selection it does not exit or
   pretend to cancel a dot task. Ctrl+Q remains the explicit exit, with existing
   pending-send warning and journal semantics.

   clipboard_bridge.rs supplies a shared injectable provider so reset/new
   textarea objects do not discard it. Ctrl/Cmd+V follows the original exact
   paste-key predicate and host read-once path, then reuses dot sanitization,
   size bounds, selection replacement and paste batch/150ms send barrier.
   Native macOS text get/set use pbpaste -Prefer txt and pbcopy, adapted from
   xai-grok-shared/clipboard.rs. The source's regular-file spool and 2s/15ms
   deadline contract is retained using a std-only 0600 create_new/NOFOLLOW,
   immediately-unlinked file. Children are detached from the foreground process
   group. No AppKit/image/tmux/OSC52 or remote clipboard probe is imported.
   Native provider construction is pure; only an explicit copy/cut/paste calls
   it. Linux/Windows keep the original in-memory provider.

   Testing never called the real system provider get/set. Unit tests inject a
   recording fake and PTY uses dev-demo InternalClipboard. Real pasteboard
   availability, native helper delivery, permissions and read-back were NOT
   verified. The original trait returns no write result, so copy delivery is not
   claimed as read-back verified. No native test is required for this delivery;
   a real clipboard-read test would need separate authorization.

3. scrollback/state/mod.rs push/push_chunk methods update entries independent
   of active pane; follow_mode/structural anchors control presentation separately.
   Dot uses a bounded GET latest-page transport, not Grok's agent stream. The
   follow_latest condition was removed from its 10-second polling predicate.
   Latest replies now merge/deduplicate by message ID instead of replacing the
   loaded older page. Old-message ID and intra-message visual-row anchor are
   restored after background layout. Anchor capture can use the last actually
   displayed prepared snapshot while new composer geometry is still pending;
   this does not enable stale rendering or hit targets.

   Timestamp sorting is stable on ties. Older pages prepend before merge so
   equal/missing timestamps preserve server-page order rather than lexicographic
   ID reordering. Expanded-message state is unaffected. Empty latest pages do
   not erase known history or declare older-history exhaustion. Failures preserve
   data and stop automatic requests; explicit refresh is still required to retry.
   At 288 records the anchor neighborhood and latest 32-message tail are kept;
   omission is reported rather than pretending the entire history is present.
   The dev-only --demo-incoming fixture injects one synthetic message on the next
   latest-page read so an actual 10-second background poll can be tested offline.

## Preserved safety and runtime cleanup

The CSI-u provenance patch, actual flags=11 negotiation, legacy-CR-as-newline,
Shift+Enter routing, request UUID/idempotency payload, unique room binding,
journal and uncertainty locks remain. Their implementation files are checked
byte-for-byte against the prior source. No transport endpoint or credential
handling was changed. Normal exit pops the keyboard flags while its alternate
screen is still active, then restores focus/mouse/terminal state. The PTY checks
this ordering and termios restoration.

## Actual validation

Default, dev-demo and release suites: 155 passed each, zero failures.
Fmt and strict all-targets application clippy: passed in both configurations.
The same six existing vendored crossterm warnings remain; no zero-warning claim
is made for the third-party package. Mac arm64 ordinary release and offline demo
release were built. No new dependencies were added.

The new unit regressions cover FocusLost/Gained, modal/busy/uncertain restore,
queued Enter and drag cleanup, pane/modifier page ownership, Ctrl/Cmd+C selection
copy, no-selection Ctrl+C, shared provider through clear/acceptance, one-read
sanitized bounded paste, inactive/locked provider guards, polling away from the
bottom, ID dedup/empty/error/cap behavior, focus/draft/uncertainty retention and
anchor capture during pending geometry.

Offline PTY verifies actual CSI focus sequences and cleanup, Ctrl+C not exiting,
in-memory copy -> reset -> paste, focus loss blocking input, focus regain with
queued encoded Enter not sending, and a real 10-second poll while parked on old
history. Incoming data is merged, the compared visible rows stay identical, and
history End then reveals the synthetic new message. The earlier three protocol
cases (confirmed/no response/partial flags) were also rerun through the actual
parser and all passed. No real clipboard data or real messages were used.

## Practical limits

Native clipboard execution is compiled but not tested against the real system.
The explicit text helper can take up to two seconds; image and remote clipboard
paths are absent. Scrollback word/paragraph selection is still outside this
composer-copy scope. PageUp/PageDown in Prompt have no invented editor behavior;
only original textarea bindings are used. New history is bounded 10-second
polling: a latest page can miss intermediate records if more than 32 messages
arrive between polls. It is not a realtime subscription/full-history guarantee.
The existing ID/visual-row anchor clamps the row ordinal after reflow rather
than promising the same source character or pixel. No native Terminal window,
image workflow, real account, global keyboard permission request or GitHub write
was used. There were no denied or pending actions blocking this repair.
