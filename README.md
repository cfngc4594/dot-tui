# dot-tui — native Rust chat prototype

Experimental terminal client for an existing ChatGPT dot. Not an OpenAI product.
This is an independent Rust/Ratatui application, not the Grok Build runtime and
not a Python application bundled into an executable. Version 0.8 opens the real single conversation directly, without a repeated
startup warning or yes/no gate. It retains the actual
Grok Build Markdown renderer, layout/prompt chrome and scroll normalization.
Development simulations are not part of the ordinary release build.

## Status

- Default: **real dot connection** as soon as you run the interactive application.
  No repeated startup warning or yes/no gate. Existing official Codex login is used.
  The composer is enabled; Enter sends and Shift+Enter inserts a newline when distinguished by the terminal. Use `--read-only` for read-only diagnostics.
- Ordinary release builds have no fake backend or demo option. Tests remain offline.
- Chinese/multiline input; one conversation, immutable sent messages; history paging and automatic refresh,
  10-second latest-page refresh (paused after errors; continues while viewing older pages).
- Image attachments: type `@` to select a local image, paste an explicit image
  path, or Ctrl/Cmd+V a macOS clipboard image/file URL. The original Grok image
  chip/preview appears locally before upload; only an explicit safe Enter sends.
  Click `[Open Image]` to fetch the received image into the terminal-only preview.
  Non-image attachments, approval cards and tool results still use the official app.
- The Rust real-account send path has **not** been tested. Fake backend tests are
  not evidence of a delivered real message. No tokens or real messages were used
  during development of this version.

## Distribution preparation

Version 0.8.0 macOS arm64 packaging and a fixed-version one-line installer are
prepared in code, but no release has been published. See
[docs/DISTRIBUTION.md](docs/DISTRIBUTION.md) for the builder, future installation
command, license inventory, existing-login requirement and unverified platforms.

## Run

An executable is platform-specific. Linux x86-64 binaries do not run on a Mac.
Build and test a macOS binary natively before distributing it. Rust binaries do
not require Python, but use normal OS system libraries.

```sh
./dot-tui
```

Keys:
- Enter: send once; Shift+Enter: newline when the terminal distinguishes that key.
- Shift+Enter: newline. A trailing backslash followed by Enter also continues on a new line.
- Mouse/trackpad wheel: scroll history. At the top, scrolling up again loads an earlier page without moving the reading anchor.
- PgUp/PgDn: scroll. Esc cancels a pending-exit warning.
- Ctrl+U: clear an unsent draft. There is no dot selector.
- Ctrl+H: earlier page or explicit retry after a paging failure; background refresh is automatic.
- Ctrl+Q: quit. Ctrl+C clears the draft; Cmd+C copies a selected composer region. While a send is pending, an additional quit confirmation
  warns that closing the app cannot retract the request.
- Bracketed paste never triggers sending or hotkeys. An absolute image path or
  file URL stages that image locally; other paste content remains text.
- `@`: image selector (arrows, Enter/Tab or mouse to select, Esc to dismiss).
  Selecting never uploads. Click `[Open Image]` or a sent image chip for a terminal-only image preview.
  Esc/q or clicking outside the fitted preview returns to history. No system image viewer is launched.
  Input-image popups retain Grok cursor/hover behavior inside the history area.
  Pasting shows a green `[Image #N]` chip and preview; typing past it hides the
  preview. Clicking the chip reopens it. Sent user-message chips preview on
  hover and hide when the pointer leaves; clicking opens the same fitted terminal preview.

The composer supports grapheme-aware Left/Right, Up/Down, Home/End, Ctrl+Home/End,
Backspace and Delete, with a visible caret and cursor-following soft wraps.
Scrollback text selection and non-image rich media remain unimplemented. Composer selection uses the original textarea. Markdown uses the original Grok
renderer, including code syntax highlighting. Draft text has no client-side character-count cap.
The right scrollbar appears only when history overflows; click or drag to seek.
It shares the wheel's scroll offset, and becomes dimmer while following the bottom.

## Build

Use Rust 1.89 or later from the official Rust distribution (vendored upstream
uses edition 2024 and let-chains). A native C compiler is needed for Oniguruma:

```sh
cargo build --release --locked
./target/release/dot-tui
```

Dependencies are resolved by Cargo.lock. The transport uses Rustls rather than
system OpenSSL. `cargo test` uses synthetic credentials/payloads only.

## Real connection (read SECURITY.md before use)

```sh
./dot-tui
# Optional read-only mode:
./dot-tui --read-only
```

The app uses an existing official Codex ChatGPT login. It never asks you to paste
an access token. Missing/expired/unsafe login files produce an error rather than a
simulated conversation. Launching the application starts the real connection;
read SECURITY.md before use. Nothing is sent merely by opening the composer.

Loads `$CODEX_HOME/auth.json` or `~/.codex/auth.json`; `--auth` accepts a path, never
an inline token. Only ChatGPT login is supported. Unix file ownership/private
permission checks apply. Windows Live mode is refused until ACL validation exists.

Sending works only for existing message rooms; initialize a conversation in the
official app first. No room creation, credential refresh or other mutation APIs.
A server-accepted message ID is **not** confirmation of delivery or dot completion.
Sending may trigger real tasks by your dot. Review the text and recipient carefully.

## Uncertain sends and crash recovery

Before a real send, a private durable marker is created at
`~/.dot-tui-send-state/pending-send`. It contains only the random request key,
not the message, account, token or transcript. One UUID is used for both server
idempotency fields. Requests are not automatically retried, including protocol
retries. A confirmed acceptance or definite local rejection clears the marker.

Network/HTTP/parse errors are conservatively uncertain. The composer is locked;
refresh remains available. The marker also locks sending after a process crash
or exit during submission. **Do not immediately resend.** Check the official app
and wait for the original result. After independently reconciling the outcome,
quit the TUI and move the exact `pending-send` file to a private backup manually
before restarting. This is a user-controlled recovery step, not an auto retry.
Drafts are not saved on exit; the app cannot restore their text after a crash.

## Loading and diagnosis

Connection setup runs in the worker, not before the interface appears. The UI
shows the current phase and elapsed seconds, and retains the last completed page
count while refreshing. Dot-list and message-page requests are sequential.
Each service request retains a 20-second transport timeout and a 10-second
connection timeout. There is no extra 30-second UI/worker phase deadline:
queueing and multi-image uploads may legitimately span several requests.
A disconnected worker is reported visibly; pending/uncertain sends retain the
durable marker and are never automatically replayed. Actual read failures use
the existing bounded-backoff scheduler.

On exit, a content-free result summary remains in terminal scrollback. No tokens,
account IDs or message bodies are printed. The app does not create diagnostic logs.
If multiple dots are returned, this single-conversation client refuses to guess
which recipient is intended. A missing room is distinct from an empty page.

## Pagination boundaries

Each request asks for `limit=32`; every returned item is considered. Latest pages
merge by message ID. `Ctrl+H` or scrolling upward at the top requests earlier
messages with a message-ID `before` cursor. Loaded history, message text and
attachment lists are no longer truncated by client-side record/character/count
limits. Text control-character sanitization and attachment-format validation remain.
The available protocol evidence does not establish `has_more` or `next_cursor`.
Short/filtered/duplicate pages are not treated as EOF. An empty page, missing
cursor, unchanged cursor or repeated cursor pauses automatic older-page requests
and offers explicit Ctrl+H retry, without claiming the history is complete.
Content remains in memory; memory/work grows with loaded history.

## References

Grok Build supplies the native Rust/Ratatui/Crossterm stack reference, input/render
separation, and an attributed terminal-control sanitizer adaptation. The coding
agent runtime is not embedded. chatgpt-dot-cli is the unofficial protocol source.
See THIRD_PARTY_NOTICES.md for exact commits and licenses.

## Development-only offline tests

```sh
cargo test --locked
cargo test --locked --features dev-demo
# Isolate developer binaries from the production target directory:
cargo build --locked --features dev-demo --target-dir target-dev
./target-dev/debug/dot-tui --demo
```

Do not distribute the developer executable as the normal product. Production is
`cargo build --release --locked` with no features. No ordinary invocation falls
back to fake replies. `--demo` is rejected by the production CLI.

## Verification record

See [docs/VALIDATION.md](docs/VALIDATION.md) for version 0.7 test results and
explicitly untested live-account behavior.

## Original input component and response correction

The composer now uses Grok Build's original `xai-ratatui-textarea` crate at commit
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`, including its editing, wrapping,
caret positioning and in-memory clipboard logic. Its 13 Rust source/test files
are unchanged; the standalone manifest only resolves workspace dependencies.
`src/editor.rs` has been removed. Dot retains its own transport, recipient checks,
explicit-Enter send boundary and terminal-control sanitization.

Prompt geometry follows `PromptStyle::default`: no accent column, left/right
padding 2/1, prefix width 2 and top padding 1. A focused empty prompt has no
placeholder. GrokNight prompt/prefix/border and OSC 12 cursor-accent colors are
used; cursor shape stays with the terminal profile. The original CursorState
policy avoids restarting blink on unchanged frames. Full Grok agent/network
startup and shell completion are not run. Image chips and the separate explicit
macOS image clipboard adapter are described in IMAGE_PORT.md.

Typing is preserved before connection results arrive; unknown/ambiguous recipients
cannot be sent to. History Markdown/layout runs on a bounded background worker;
only visible prepared rows are copied during painting. Input batches are bounded
and stop at send/exit-state transitions. Grok's original paste coalescing
functions are extracted with explicit dot command boundaries.

Development-only offline timing fixtures: `--demo --demo-delay-ms 1200` and
`--demo --demo-history 288`. These flags are absent from the normal release and
never read real login files. See `docs/INPUT_RESPONSE_VALIDATION.md` for measurements
and limits; the existing release-license and validation documents are retained.


## Enter, focus and history interaction

Click the composer to enter input; click history or other chrome to leave input.
In history focus, Space (also Tab) enters the composer and consumes that key;
Enter cannot send while history is focused. The focused empty composer hides
its placeholder; unfocused content uses the original 0.66 fade and hides the caret.
The footer follows focus (Space:输入 versus Enter:发送 / Shift+Enter:换行).

Enter routing is extracted from PromptWidget: a preceding backslash continues
on a new line. Shift+Enter always inserts a newline. Enter sends only after a
real query/push/query handshake confirms DISAMBIGUATE_ESCAPE_CODES,
REPORT_EVENT_TYPES and REPORT_ALL_KEYS_AS_ESCAPE_CODES, and only when the event
came from a CSI-u encoded Enter. The crossterm 0.28 adapter preserves this source
marker; raw CR remains distinct and ALWAYS inserts a newline, even after a
successful handshake. If a terminal cannot confirm these flags, sending is
disabled and the footer says so; a TERM/terminal-brand name never grants support.
modifyOtherKeys is not enabled or assumed to be sufficient. Dot does not use a
current CoreGraphics snapshot to reinterpret queued keys. The original no-permission
probe was tested separately but did not return in this environment and was stopped.
IME candidate/preedit state is handled by the terminal; crossterm
only exposes committed events. Bracketed-paste newlines do not submit. Paste and
focus transitions suppress submission for the whole collected batch; a short
post-paste guard also protects a following burst. Repeat Enter is ignored, and
an unchanged draft cannot be resubmitted after a definitive rejection without
editing it. Network failures are never automatically retried.

History opens at the latest page's bottom. Scrolling away disables follow;
scrolling upward again at the top requests one older page. Existing message ID
and intra-message visual row are restored after asynchronous layout, including
resize (the same visual-row ordinal is kept and clamped after reflow). Empty or
non-advancing pages pause automatic paging and permit explicit Ctrl+H retry;
older-page errors retain an explicit retry path while background latest-page reads continue. No total-history record cap is applied.

Long user messages use original user-block foldability (estimated visual lines
at 60 columns >3), default to three visible rows plus ellipsis, and toggle on a
300ms same-entry double-click. Assistant/unknown messages do not use this fold
policy. Movement of one row/column cancels the click, and a third click does not
toggle a user prompt again. Expanded state is keyed by message ID across refresh.
As upstream, toggling a user prompt anchors the clicked entry's top. Dot fixes
selection policy to upstream's default Flash; the WordSelect precedence branch
is retained and tested not to fold, but remote appearance settings and scrollback
word/paragraph selection are not exposed by this adapter.

The scrollbar track uses the outer screen's right edge with upstream default
ScrollbarConfig gap_left=0/gap_right=0; content retains two-column outer padding.
The draw and pointer paths share that rectangle. This is source/buffer validation,
not a claim of identical terminal fonts, external margins, or pixels.


## Focus events, clipboard and live history polling

The terminal emits focus events through EnableFocusChange; normal/panic cleanup
disables them. FocusLost hides the caret, cancels drag/scroll latches and prevents
queued keys/mouse/paste from operating on the inactive application. FocusGained
reasserts mouse capture and follows the original non-vim idle/no-modal predicate
when returning from Scrollback to Prompt. It does not enable follow-bottom or
submit a queued Enter. No clipboard/image probe runs on focus changes.
PageUp/PageDown/Home/End belong to history when Scrollback has focus; modified
page keys remain with the textarea. Dot uses the original prompt route without
the optional force-conversation-paging mode (there is no dropdown/history panel).

A shared ClipboardProvider survives draft clear and send acceptance. Ctrl+C
clears the composer; Cmd+C copies its selection. Ctrl+Q remains exit.
Ctrl+V/Cmd+V reads the provider once at explicit paste intent and uses sanitized,
bounded insertion plus the same paste-batch send barrier. macOS live mode installs
the native pbcopy/pbpaste text provider adapted from Grok's shared clipboard;
construction, focus and polling do not read it. Text is spooled through private,
immediately unlinked files and helper children have a two-second deadline.
The system provider was compiled but not exercised on the real clipboard:
unit tests inject a fake; dev-demo/PTY use a shared InternalClipboard. Linux and
Windows keep the in-memory provider; OSC52/tmux paths are not imported.
Explicit macOS image/file-URL paste is a separate background adapter and has
only been validated with injected synthetic data, not a real clipboard read.

Latest GET polling continues while reading older history. Incoming pages merge
by message ID instead of replacing loaded history. Reading ID/visual-row anchor,
focus, draft and send uncertainty remain independent of fetching; async layout
restores the anchor before displaying the new revision. Loaded messages are not
evicted to enforce a record-count window. A single latest-page poll can still
miss intervening messages between polls; polling is not a realtime subscription
or complete-history guarantee. Transient read errors automatically retry with backoff; successful reads restore the normal background cadence.


## Image attachments

Image validation, atomic chips/undo, image popup layout and Kitty encoding/cleanup
come from Grok Build fixed 2bdd1d6. See IMAGE_PORT.md for exact retained files and
host adaptations. The selector reuses the original @ parser and RGB dropdown,
with a bounded nonrecursive dot index: up to 1024 directory entries, 64 results.
It filters local image filenames in the working directory or explicitly typed
directory; it does not import the whole Grok workspace index/agent runtime.
Paths with spaces can be selected from the dropdown or pasted as a quoted path.

Encoded files are limited to 20 MiB, preview dimensions to 8192 per side / 16 million
pixels, decoder allocation to 128 MiB, and the draft to ten images. Validation and
pixel preparation run on the worker. Unsupported terminal image protocols show
the original metadata fallback; iTerm2 remains disabled by the original policy.

Room metadata uses the authenticated fixed chatgpt.com client. Downloads use a
fresh anonymous client with redirects/proxies/cookies disabled, public IP checks
and DNS pinning. No credentials go to image download hosts. HTTP failures and
non-image/oversized responses are shown as errors without automatic retries.
Download URLs and the real account upload path have not been exercised against
a real account. Fake-service/PTY tests prove local behavior, not real delivery.

Development-only synthetic paste fixture:
`cargo run --features dev-demo -- --demo --demo-clipboard-image`, then Ctrl+V.
It cannot read the system clipboard. The separate `--demo-image` popup harness
also remains available. Neither option exists in production builds.


## History navigation and media follow-up

The centered `▼` in the one-row status strip returns to the latest messages.
It is shown only while there is content below and disappears when following.
The surrounding two blank rows were removed; prompt/footer geometry stays separate.
History page insertion preserves the message/visual-row anchor before applying
queued wheel deltas. Painting and hit targets also resolve that anchor against
one immutable layout snapshot to avoid a one-frame jump to a newly prepended top.
Width changes preserve the message and clamped visual row; this does not import
Grok's entire logical-line mapping system for arbitrary Markdown reflow.

History attachments use Grok's `[Open Image]` label, centering and color, then its
terminal-only fitted image-preview flow (the earlier native-open routing is removed).
Dot still adapts signed remote metadata/downloads to private session cache files,
while Grok's original image-overlay geometry and terminal placement render
the preview. The 90% viewer frame/close button are no longer used on this path. Cache admission, anonymous download validation and cancellation remain
in force. No program or visual validation was run for this update.


## Background connection recovery

The footer has no manual refresh control or Ctrl+R refresh shortcut. New messages
are checked in the background every 10 seconds. Transient latest-page/list read
errors retry after 10, 20, 40, then 60 seconds; a successful read resets the delay.
An initially unavailable login file is re-opened by subsequent connection attempts.
Expired active-session credentials require official login and restarting this client;
ambiguous multiple-dot selection and a disconnected worker remain explicit errors.
No send operation is automatically replayed. Failed/non-advancing older-history
loads expose only a contextual Retry history control and Ctrl+H, not a permanent
replacement for refresh. Runtime recovery has not been exercised for this build.

## Status placement and official-CDN fake-IP handling

Connection, send, image work, layout and errors share the top-right header.
The input border and bottom arrow row no longer repeat connection results;
normal idle reads show no permanent ready/connected message. Completion notices
expire after four seconds; unresolved visible-image errors remain in the header.
Image rows retain the image chip and Open Image action, not duplicate diagnostics.

The image downloader still rejects private, reserved and fake IP destinations.
If local DNS returns only 198.18.0.0/15 addresses for an oaiusercontent.com host,
it queries Cloudflare's documented HTTPS DNS JSON endpoint for that hostname's
public A records. The resolver endpoint is pinned to its documented public IPs;
answers must pass the same public-IP check and are pinned for the image request.
The resolver receives no signed image URL, URL query, account headers or cookies.
TLS verification, HTTPS/443, anonymous download and no redirects remain enforced.
Other non-public resolutions remain rejected. This fallback has been compiled
and statically reviewed; no account download or GUI validation was run.

## Interface language

Type `/` in an empty editable composer to open the Grok-style slash dropdown.
Its only command is `/language`. Enter or Tab opens the English / 中文 choices;
Up/Down or Ctrl-P/N wraps selection; wheel and PageUp/PageDown stop at the ends.
Mouse clicks and Tab complete a command/argument; Enter confirms the language.
Esc or an outside click dismisses. Confirming a language saves the preference
and clears only this local command; it never submits a chat message. Direct
`/language en` or `/language zh` also selects a listed language with Enter.

First launch defaults to English. The selected value is stored atomically in
`~/.dot-tui/language` (`en` or `zh`, private directory/file). A missing or invalid
preference uses English; unsafe/unwritable preferences produce a UI error when
saving. No account, token, draft or message content is stored with this setting.
The menu follows the existing editable-composer availability rules.

Language covers the menu, active status/progress, shortcut footer, exit dialog,
image preview labels/actions and bundled client diagnostics. User/dot message
bodies, pasted paths, identifiers, MIME names and canonical `[Image #N]` tokens
are preserved. Unknown low-level diagnostics remain in their original language.
The source catalog has 222 bilingual entries; this is not a claim of runtime
interaction, persistence, or visual validation.

User-message folding now preserves the captured entry/scroll anchor through
async relayout instead of navigating the entry to the top. Collapse corrects
only when the visible fragment disappears or scroll limits require clamping.

The slash panel now also uses Grok's original `render_dropdown_chrome`: top and
bottom dividers, two-column row inset, count hint and original RGB border/band
styles. Prefix coloring and muted completion suffix use the original textarea
screen-coordinate path; the real input caret stays at the insertion point.
English / 中文 arguments use the native-name plus `(active)` marker convention
from Grok's ThemeCommand suggestions, rather than a separate settings widget.


Clipboard follow-up: Ctrl+V/Cmd+V forwarded by the terminal uses Grok's native
macOS raster reader with its original file-URL/AppleScript fallback. Terminal
bracketed paste uses Grok's payload classification, clipboard-text comparison and
change-count fence before attaching a raster. Short plain-text pastes with no
advertised raster do not run the attachment helper. A scrollback-focused paste
returns focus to the editable composer, following Grok's FocusPrompt+forward.
Local staging uses a separate bounded serial worker rather than waiting behind
Dot HTTP calls. No clipboard read uploads or sends an image. Missing content,
locked input, read failures and a full staging queue produce top-right feedback.
The terminal must deliver a paste key or bracketed-paste event; a system paste
shortcut swallowed by the terminal cannot be observed by the TUI. These paths
were compiled only for this follow-up; no live clipboard or GUI check was run.


Prompt preview event ownership follows Grok's full widget/parent split: pointer
motion updates pane/button/link hover and does not count as a prompt interaction.
The newly pasted image therefore remains previewed while the mouse moves.
Clicks in another pane hide it through prompt focus; typing after the chip clears
the just-inserted preview. The existing original cursor-on-chip/right-edge rule
shows it again when deletion or navigation returns the cursor to the image chip.
Actual prompt clicks, key edits, explicit cursor changes and text paste use the
original latch transitions. This follow-up was inspected and compiled only;
no actual preview interaction was executed.
