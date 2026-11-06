# Shift+Enter ambiguity and safe submission boundary

## Root cause and capability evidence

The prior build routed every unmodified KeyCode::Enter to submit while passing
false to PromptWidget's Apple modifier rescue. crossterm 0.28.1 translates raw
legacy CR and CSI-u Enter into the same KeyEvent shape, so a terminal that maps
Shift+Enter to CR could accidentally send. TERM/TERM_PROGRAM names cannot prove
that the events are distinguishable.

The official cached crossterm 0.28.1 source supports progressive keyboard queries
and Push/PopKeyboardEnhancementFlags, but its boolean probe discards actual flags
and may wait 2 seconds. The application previously did not use it. During this
fix a second parser defect was reproduced: the 0.28 flags decoder reads the first
ASCII byte as bits, so a real decimal `11` reply becomes flag 1. A decimal parser
is required before actual multi-flag confirmation can be trusted.

Grok source was checked at fixed 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8:
macos_modifiers.rs calls CGEventSourceFlagsState with no permission request API;
terminal_support.rs gates rescue by capabilities and suppresses it for queued
events via a scoped guard. This is distinct from reading Terminal windows.
The original standalone probe compiled in the normal sandbox, then did not
return a modifier snapshot; that experiment was interrupted (exit 130). No
permission denial occurred, but runtime availability has an uncertain outcome.
It was not retried and is not used as evidence to authorize sends. A current
snapshot, even when available, cannot prove the modifier state of an earlier
queued CR; this patch never applies current global state to queued events.

## Implemented safety boundary

1. Uses a bounded actual-flags query, Push(11), then another actual-flags query.
   DISAMBIGUATE_ESCAPE_CODES, REPORT_EVENT_TYPES and REPORT_ALL_KEYS_AS_ESCAPE_CODES
   must all be confirmed. Queries target the validated TUI stdout, not a different
   controlling /dev/tty. Each query has a 250ms deadline plus at most 20ms to drain
   the paired device-attributes response. Other queued key events are retained.
2. A pinned crossterm adapter preserves CSI_U_ENTER in KeyEventState only for
   CSI-u Enter. Raw CR and modifyOtherKeys events remain unmarked. This is a
   minimal source patch, not a claim of unmodified upstream crossterm.
3. Enter can submit only when the session is confirmed AND its source is marked.
   Raw/unknown CR always becomes a newline, even in a confirmed session. Encoded
   Shift+Enter becomes a newline through the original textarea. Unknown sessions
   force all Enter events to newline and display '发送已禁用'. modifyOtherKeys is
   not enabled or trusted as confirmation. Alt+Enter is not the advertised chord.
4. Only this session's own pushed flags are popped on drop, including safe
   fallback after a partial flags reply. No TERM/brand guess enables submission.
5. Original PromptWidget continuation/router ordering is retained. Encoded Enter
   is a command boundary in the original paste coalescer. Existing paste/focus
   batch guards, repeats, room binding, journal, uncertainty lock and no-retry
   transport remain. No journal or unknown-result state is cleared by negotiation.

The fixed behavior is Shift+Enter newline / Enter send on a confirmed enhanced
terminal. A legacy terminal cannot safely provide both from indistinguishable
CR bytes: sending is intentionally disabled there, with a clear persistent
caption/footer. The actual user's refused Terminal window was not accessed;
its real hardware key mapping/capability was not tested. Runtime negotiation
will decide without account access or permission prompts.

## Validation

Default/dev-demo/release suites each passed 142 tests. fmt check and strict
all-targets clippy passed for the application in both feature modes. The newly
vendored upstream crossterm reports six existing compiler warnings (cfg names,
parentheses and unused internal filter); these are dependency warnings, not
zero-warning claims for that third-party package. Source parser regression
cases are included; actual parsing/handshake is also exercised by the PTY harness.
The full unrelated upstream crossterm development test suite was not run.

Offline PTY cases use a controlled emulator replying with flags 0 then 11,
no reply, and flags 0 then 1. They use deliberately conflicting TERM names,
proving names do not grant support. Tests verify encoded Shift+Enter, encoded
plain Enter, raw CR even after confirmation, repeat/double-Enter, bracketed-paste
followed by an encoded Enter, disabled-send UI, protocol stack pop and termios
restoration. These are synthetic capability tests, not a real Terminal-window
or physical keyboard claim. Logs and exact binary SHA/time are in validation.json.

An initial PTY run safely downgraded because it exposed the decimal flags parser
bug; the parser was fixed. The harness was then adjusted to wait for the actual
footer (instead of assuming process startup time), and to use the tested TUI's
stdio rather than a separate controlling-session lifetime that revoked the test
PTY before termios inspection. Failed experiments did not send real messages.

No real credentials/accounts/messages, no refused Terminal window access or
workaround, no global keyboard permission request, and no GitHub mutation.

## Deferred outside this urgent fix

Ctrl+C vs textarea copy/ClipboardProvider, focus-aware PageUp/PageDown,
FocusChange events and receiving new messages while reading older history were
reported by the parent as separate audit items. They are not expanded here.
