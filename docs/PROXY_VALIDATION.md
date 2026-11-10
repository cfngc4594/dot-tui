# Environment proxy validation

Validated on macOS arm64 on 2026-11-10 against the existing working tree.
This change follows the user's final choice to reuse Grok Build / reqwest proxy
**environment-variable behavior**, not to implement native system-proxy lookup.

## Passed

- `cargo test --locked --offline`: 505 passed, 0 failed.
- `cargo test --locked --offline --features dev-demo`: 506 passed, 0 failed.
- `cargo fmt --check` and `git diff --check`.
- `cargo build --locked --offline`: default-feature debug candidate built in a
  separate target directory, not installed over the normal executable.
- Candidate `--help`: `--no-proxy` and `--read-only` present; development-only
  `--demo` and `--showcase` absent.
- Live, read-only candidate through a local HTTP CONNECT proxy selected by
  `HTTPS_PROXY`: dot-list GET returned HTTP 200 (5899 ms); message-list GET
  returned HTTP 200 (968 ms). No message was sent, no image was uploaded, and
  no account token, account identifier, or conversation content was printed or
  saved by the test harness. Terminal output was discarded. Only the existing
  allowlisted stage/status diagnostics were inspected for the test process.
- The installed executable's SHA-256 was unchanged before/after validation.
- Cargo.toml and Cargo.lock are byte-identical to their pre-task versions. The
  abandoned native-proxy dependency experiment is not part of the change.
- Existing README/showcase/selection/focus/i18n/UI work was preserved. Among the
  saved pre-task files, only src/backend.rs, src/main.rs, and SECURITY.md changed;
  proxy tests and these two proxy documents are new files.

## Checks not passing or not performed

- Strict `cargo clippy --locked --offline --all-targets -- -D warnings` failed
  on 23 existing findings outside the proxy changes: diagnostics, i18n, image
  composer/ingress, scrollback, scrollbar, UI tests and unchanged main/UI code.
  Examples include nested-if style, regex construction in a loop, redundant
  casts, and comparisons with an unlimited image cap. These unrelated files
  were not altered to make the proxy change look fully lint-clean.
- Vendored crossterm emits existing compile warnings.
- No native Windows/Linux runtime validation, optimized release build, release
  packaging, installed-app replacement, commit, push, or publication was done.
- No real send/upload or proxy-only image-download success was tested. Anonymous
  image downloads deliberately remain outside this proxy change.
- The live result verifies the tested local proxy/environment at that time; it
  does not guarantee future network availability or untested proxy types.

## Trial

Launch the separately built candidate with `HTTPS_PROXY` set to the endpoint of
an existing trusted HTTP/HTTPS proxy. Do not assume an unchanged installed
`dot-tui` command runs this candidate. Use `--read-only` for a read-only trial;
use `--no-proxy` only for deliberate direct access. No system network setting or
shell startup configuration was changed.
