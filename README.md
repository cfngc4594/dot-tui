# dot-tui — native Rust chat prototype

Experimental terminal client for an existing ChatGPT dot. Not an OpenAI product.
This is an independent Rust/Ratatui application, not the Grok Build runtime and
not a Python application bundled into an executable. Version 0.7 opens the real single conversation directly, without a repeated
startup warning or yes/no gate. It retains the actual
Grok Build Markdown renderer, layout/prompt chrome and scroll normalization.
Development simulations are not part of the ordinary release build.

## Status

- Default: **real dot connection** as soon as you run the interactive application.
  No repeated startup warning or yes/no gate. Existing official Codex login is used.
  The composer is enabled; every message still requires separate review and
  confirmation. Use `--read-only` for read-only diagnostics.
- Ordinary release builds have no fake backend or demo option. Tests remain offline.
- Chinese/multiline input; one conversation, immutable sent messages; history paging, manual refresh,
  10-second latest-page refresh (paused after errors / while viewing older pages).
- Only text. Attachments, approval cards, tool results and full rich rendering must
  be handled in the official app.
- The Rust real-account send path has **not** been tested. Fake backend tests are
  not evidence of a delivered real message. No tokens or real messages were used
  during development of this version.

## Run

An executable is platform-specific. Linux x86-64 binaries do not run on a Mac.
Build and test a macOS binary natively before distributing it. Rust binaries do
not require Python, but use normal OS system libraries.

```sh
./dot-tui
```

Keys:
- Enter: insert a newline (never sends).
- Ctrl+S: review the current draft. Y: confirm that exact draft/recipient once.
- Mouse/trackpad wheel: scroll transcript or review. Click buttons for actions.
- Esc: cancel review. PgUp/PgDn remain optional keyboard alternatives.
- Ctrl+U: clear an unsent draft. There is no dot selector.
- Ctrl+R: latest page; Ctrl+H: earlier page.
- Ctrl+Q / Ctrl+C: quit. While a send is pending, an additional quit confirmation
  warns that closing the app cannot retract the request.
- Bracketed paste is handled as text; pasted commands cannot trigger hotkeys.

The first editor is intentionally append/backspace based; full cursor selection,
rich media and full cursor selection are not implemented. Markdown is rendered
with the original Grok renderer, including code syntax highlighting. Maximum draft 16,000 chars.

## Build

Use Rust 1.88 or later from the official Rust distribution (vendored upstream
uses edition 2024 and let-chains). A native C compiler is needed for Oniguruma:

```sh
cargo test --locked && cargo build --release --locked
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
count while refreshing. Dot-list and message-page requests are sequential: each
request has a 20-second transport deadline; each worker phase has a 30-second UI
watchdog. Thus 30 seconds from startup is not proof of service refusal.

A disconnected or overdue worker is reported visibly; automatic polling stops,
new sends are disabled, and the process must be restarted. A pending send remains
uncertain and its durable marker is preserved. Late worker results are ignored.
The watchdog does not claim to cancel an in-flight network request.

On exit, a content-free result summary remains in terminal scrollback. No tokens,
account IDs or message bodies are printed. The app does not create diagnostic logs.
If multiple dots are returned, this single-conversation client refuses to guess
which recipient is intended. A missing room is distinct from an empty page.

## Pagination boundaries

Each request reads at most 32 messages. Latest refresh replaces the visible page;
it does not claim all intervening messages were fetched. `Ctrl+H` reads earlier
pages. A session is bounded to 288 messages and 16,000 displayed chars per message.
Cursor ordering uses returned timestamps/message IDs, not an archival export API.
Refresh to return to the newest page. Errors stop unattended polling until manual
refresh. Content is never saved to disk.

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
