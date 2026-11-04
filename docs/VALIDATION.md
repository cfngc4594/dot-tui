# Version 0.7 verification

Verified source archive SHA-256:
`c3365bede8e24aad60337648d957dc1af16e0ca60d0444ad0b6c0261bf6b3e60`.
The published Rust/Cargo sources are byte-identical to that validated archive.
Publication additionally updates documentation and third-party license notices.

## Native macOS arm64 verification (2026-11-04)

- Default-feature tests: 97 passed.
- `dev-demo` feature tests: 97 passed.
- Release build produced a native Mach-O arm64 executable.
- Development-only offline terminal test: starts without a `yes` gate; Chinese
  input, Enter inserting a newline, draft clearing and exit passed.
- No real-account read or send was executed. Offline success does not establish
  a working live connection or a delivered message.
- Screenshot comparison was not performed because that step was denied.

## Cloud verification and limits

- `cargo check --locked --tests --features dev-demo` and formatting checks passed.
- Cloud root test compilation stalled and was stopped; it is **not** reported as
  97 tests passed on Linux.
- 32 unmodified upstream Grok files were compared against the recorded SHA-256
  manifest during publication; all matched.
- Publication review found no credentials or private conversation data. The
  development artifacts and obsolete early validation report are excluded.

Not verified: live authentication and message delivery, native Windows operation,
full Grok visual parity, approval cards, attachments, or hosted CI for this commit.
