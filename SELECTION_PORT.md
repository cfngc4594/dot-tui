# Grok message selection port

## Source and permission

On 2026-11-10, `git ls-remote https://github.com/xai-org/grok-build.git HEAD refs/heads/main`
returned `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` for both. That public
snapshot identifies xai-grok-pager as 1.0.45. The host checkout is dot-tui 0.8.6,
base commit `c52d9233658c6f5a91a48f7f24848324a20fffe0`.

Grok Build is Copyright 2023–2026 SpaceXAI, Apache-2.0. The retained license is
`licenses/Grok-Build-APACHE-2.0.txt`; source headers mark modifications. The
upstream root has THIRD-PARTY-NOTICES, not a separate NOTICE. No new dependency,
asset, logo, agent runtime or account service is imported by this port.

## Imported code

Paths below are relative to `crates/codegen/xai-grok-pager/src/` in that commit:

- `scrollback/text_selection.rs` → `src/scrollback/text_selection.rs`:
  original resolved-range types, hit tests, pending/active/persistent selections,
  one-cell drag threshold, Unicode column snapping and slicing, text reconstruction,
  inverted selection painting, edge-autoscroll calculation, table selection and
  semantic selection helpers. Upstream git blob: `f1ae97c5291e53dfd29ac10e7731f21b01747687`.
- `scrollback/table_geometry.rs` → `src/scrollback/table_geometry.rs`:
  original table detection/cell geometry. Blob: `fb626a8e630dd4590a03c20a7c47e996233f01fd`.
- Selection-related definitions/helpers/tests from `scrollback/types.rs` →
  `src/scrollback/types.rs`. Blob: `13b0868df54bd24635610bba212da532800dc853`.
- `app/agent_view/selection.rs` and `app/mouse.rs` → single-pane integration in
  `src/history_selection.rs` / `src/main.rs`: pending promotion, captured dragging,
  range-clamped head tracking, release-copy, lost-release recovery, block drag,
  deferred text anchors, persistent WordSelect, and its original 300 ms text
  multi-click routing. Double-click selects a word/URL; triple-click selects a
  wrapped logical paragraph, table cell or whole table. Selection dispatcher
  source blob: `a1f1fe13e1f47c261bfb474c2cc632a335d1699c`.
- `scrollback/render.rs` → selection-model host adapter in `src/ui.rs`.
  Stable block-line indices and body columns exclude timestamps, prompt prefixes
  and outer decoration. Copies use the full captured body, including offscreen rows.
- The already imported original `render/wrapping.rs::word_wrap_lines_with_joiners`
  is now retained through Markdown and user-prompt layout. Soft wraps copy their
  exact skipped whitespace; real newlines remain newlines.

The extraction retains 161 upstream tests. Six tests specific to Grok's visual
BiDi renderer are omitted because dot-tui does not use that renderer.

## Host adaptations and limits

This is a bounded source port, not a replacement of dot-tui by the Grok app.

- Dot has one message pane, an asynchronous immutable layout cache, and a
  WordSelect policy. A new layout snapshot or resize cancels an old selection rather
  than applying stale line indices to new messages. Autoscroll uses a 50 ms host
  timer and the original direction/speed algorithm; pointer movement reclamps
  against the current visible model.
- Dot's renderer paints logical-order text and sanitizes control characters. The
  selection code therefore uses the upstream logical-order branches, not Grok's
  optional visual-BiDi route. Grok shell/config access is replaced by the original
  default word separators. Exact body hits follow WordSelect multi-click routing;
  non-text prefix/gutter double-clicks retain the original human-message fold
  action. No Grok-specific agent navigation, text-selection tips or telemetry
  is imported.
- Text drags stay within their original message/range, as upstream does. Starting
  on a non-text part of a block supports block selection; entering text converts
  that gesture to a text drag. Images are not text and image controls retain their
  existing click behavior. Message image chips are copyable rendered labels when
  included in a text selection, not clipboard image data.
- Table geometry and text/TSV reconstruction are reused. Dot does not provide
  Grok's rich source table-copy side metadata or hidden tool/quote boundary
  metadata; copies reflect the displayed body and detected cell geometry.
- The shared clipboard provider delivers release-copy and checks the native
  write result. On this Mac it uses local pbcopy, so it does not depend on Ghostty
  native selection or Zellij OSC 52 forwarding. Original feedback presentation
  from `app/agent_view/{notices,render}.rs` and pager-render `clipboard/mod.rs`
  appears in the bottom-right scrollback slot: `Copied!` for 1 second, or
  `Copy failed.` for 4 seconds. Both labels use the active UI language: Chinese
  shows `已复制！` / `复制失败。`; English remains unchanged. Fitting, placement
  and clipping use terminal-cell widths and keep grapheme clusters intact.
  Duration uses elapsed time rather than 30 fps
  ticks, and Grok-only `/doctor`/`/minimal` hints are omitted. Semantic copies
  retain the original 500 ms toast debounce without dropping clipboard writes
  or a first toast in a batched triple-click. The shared provider additionally
  reports composer copy results; this is an explicit host integration, not a
  claim that the public Grok prompt route emits this same toast.
- Remote/SSH clipboard delivery, OSC 52 probes, backup-copy files and full Grok
  clipboard fallback machinery are not imported. A native failure is reported
  honestly rather than being displayed as a successful copy.
- Prompt textarea selection, mouse reporting, focus events and terminal cleanup
  retain their existing routes. No terminal preferences, Zellij config, installed
  executable, current window or clipboard contents are modified for testing.

## Runtime policy and the follow-up correction

The first uncommitted port used the public source's static `Flash` fallback and
omitted the toast dispatch. That was incomplete and did not match the user's
installed Grok. This follow-up corrects both, rather than changing the mode to
`Hold` as an approximation.

The public snapshot's `pager-render/src/appearance/cache.rs:385–416,630–665`
loads explicit preferences and applies a remote soft default. Its
`pager/src/app/event_loop.rs:1609–1615` invokes that path. Therefore the static
Flash default alone is not evidence of actual Grok runtime behavior.

The locally installed Grok version is 1.0.77. Its bundled configuration reference
(`26-config-reference.md`, line 716) documents `ui.keep_text_selection` default
`word_select`; the user's local config has no explicit selection override. Dot
now uses `WordSelect`, with its public source's actual selection and multi-click
logic. The remote value itself was not queried, and public 1.0.45 source is not
represented as a byte-identical implementation of the installed 1.0.77 binary.
Selection persists after the copy notice expires. Escape, a fresh click,
keyboard/wheel navigation, focus loss, resizing or a replacement layout snapshot
cancel it. Identical background polls preserve both the layout snapshot and held
selection; only actual displayed-message changes invalidate them. Flash remains
a supported internal policy but is not Dot's default.

## History investigation

The available repository is not shallow. Its complete available 24-commit history,
all refs/reflog, seven v0.8 tags, and older local prototypes/archives were checked.
No dot application runtime in that evidence contains an implemented `Copied!`
message-selection toast that was subsequently removed.

- `bd469cb`, the earliest code commit (Rust 0.7), already lacks message-body
  text selection and a copy toast. It is the earliest observed missing state,
  not an identified regression/deletion commit. The preceding root commit only
  contains a README.
- `4707c7686a7b2c6ba2ff6522def9407a2410e39a` (0.8.0) imports the original
  textarea. Its mouse-release path copies input selection and returns
  `SelectionFinished`; Dot does not consume that return for copy feedback.
- At base `c52d923`, `src/main.rs:1932–1936` still ignores that textarea result,
  the history route only handles folding, and the native clipboard adapter
  discards write errors. `upstream_text_selection.rs` contains policy definitions,
  not the selection engine itself.
- Older local reference files contain upstream `Copied!` code, but are reference
  material, not evidence it ran in dot-tui. One earlier prototype's Ctrl+C copy
  alias changed to clear-draft in 0.8.0 while Cmd+C remained copy; that is a
  separate shortcut change, not evidence of a removed toast.

This does not disprove the user's recollection of an older executable, terminal
selection or an unavailable build. No specific deletion commit can be named from
these sources. The current missing notice and expiring highlight were errors in
this uncommitted initial port, now corrected here.

## Validation

Final validation on the connected Mac (Rust 1.96.0-nightly):

- `cargo fmt --all --check` and `git diff --check`: pass.
- `cargo test --locked --offline --target-dir target-showcase`: 499 passed.
- Same command with `--features dev-demo`: 500 passed.
- `cargo build --locked --offline --features dev-demo --target-dir target-showcase`:
  pass. The binary is `target-showcase/debug/dot-tui`.
- Host integration tests cover wide/combining/reverse selection, hard/soft
  wraps, captured edge scroll, lost-release recovery, focus/resize cancellation,
  async snapshot replacement, gutter fold clicks, image clicks and prompt
  ownership. Further tests cover persistent selection, original toast geometry
  and expiry, native failure, repeated copy, batched double/triple-click
  debounce, word/URL selection, paragraphs, table cell/whole-table TSV, and
  held-selection copy shortcuts, and unchanged background poll persistence.
  Localization tests cover both languages for history and composer copies,
  success/failure, live language switching, Chinese continuation cells, combining
  characters, emoji, narrow slots and clipped panes. They inject a clipboard;
  the locale-switching test runs in its own process to avoid test-global races.
- A direct isolated pseudo-terminal test delivered real SGR drag/release events,
  copied `这是一段`, and pasted it into the demo composer through its internal
  clipboard. It did not read credentials or write the system clipboard.
- Standard all-target Clippy is blocked by two pre-existing
  `clippy::absurd_extreme_comparisons` errors in `src/image_composer.rs` and
  `src/main.rs` involving `IMAGE_CAP == usize::MAX`. Both are present in HEAD;
  this port does not change them. Existing vendored crossterm warnings remain.
- Zellij-specific testing was stopped at the user's request. It is not marked
  passed. Actual Ghostty/Zellij visual and system-clipboard acceptance is left
  for the user's trial. No test manipulated an existing window or session.

Trial commands, from this repository:

- `./target-showcase/debug/dot-tui`: real account using the existing login.
- `./target-showcase/debug/dot-tui --demo`: offline fixture; copies use its internal
  clipboard, so press Space to focus the composer and Ctrl+V to verify offline.

For the real-account run, drag across message text and release to copy, then paste
where desired. A successful native copy shows `Copied!` / `已复制！` (following
the current UI language) for about one second;
the selection remains. Double-click body text for a word/URL, triple-click for
a logical paragraph/table cell, or double-click the non-text prompt prefix to
fold a human message. The installed executable is not replaced.

At the time of selection-port validation, existing README/showcase/assets and
the image-focus fix were preserved. No commit or push was part of that validation.
