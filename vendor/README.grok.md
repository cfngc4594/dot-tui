# Grok Build renderer source

Source: https://github.com/xai-org/grok-build

Verified source commit: `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`.
Copyright 2023–2026 SpaceXAI. Apache License 2.0. The original complete
license, including the copyright notice, is in each vendored crate's `LICENSE`.

These are actual source copies, not an independent markdown lookalike. Every
Rust source file in the two vendored crates is unchanged from that commit.
`UPSTREAM_MANIFEST.json` records the original path, git blob ID and SHA-256 of
each byte-for-byte copy. The manifest includes the original manifests as
`Cargo.upstream.toml` so the packaging changes can be compared directly.

## Copied crates

- `crates/codegen/xai-grok-markdown`: complete `src/`, its two original syntax
  assets, and the original `build.rs`. This includes the original markdown
  parser, styled renderer, GFM tables, link handling, syntax highlighting,
  Mermaid rendering, LaTeX conversion, and streaming renderer implementation.
- `crates/codegen/xai-grok-markdown-core`: complete `src/lib.rs`, including the
  original pulldown-cmark option set and the double-tilde-only strike filter.

The standalone `Cargo.toml` files resolve workspace dependency versions to
the upstream workspace's version constraints. Both retain upstream edition
2024. Workspace-only lint configuration, playground executables and benchmark
targets are omitted. The core dependency uses the adjacent vendored path.
No upstream Rust implementation or syntax asset was modified. The builds need
Rust 1.88 or newer because the unchanged upstream code uses let-chains.

## Pager rendering adapters

`src/grok_markdown/` copies the following from
`crates/codegen/xai-grok-pager-render/src/` at the same commit:

- `render/wrapping.rs`: the complete production module is unchanged, with an
  attribution header added. Its pager-specific tests are omitted.
- `render/line_utils.rs`: `line_to_static`, `push_owned_lines`, and
  `fit_line_to_width` are retained unchanged with their imports. Other helpers
  and pager tests are omitted.
- `theme/md_style.rs`: production mapping is copied, changing only the theme
  lookup from `super::Theme::current()` to `super::theme()`. The adapter returns
  the exact markdown palette and heading modifiers from `theme/groknight.rs`.
  Pager-specific tests are omitted.

`src/grok_markdown.rs` is the dot integration layer. It calls the original
renderer and syntax highlighter, supplies the GrokNight theme, wraps with the
original pager algorithm, bounds pathological viewport widths, sanitizes input
and rendered text, and maintains a cache capped at 288 entries and 32 MiB of estimated source
and rendered-output allocations. Its integration tests cover
formatting, lists, links, code backgrounds, styled wrapping, blockquotes,
tables, terminal-control safety, the original strike rule, plaintext input,
32-message cache reuse and count/output-allocation cache limits.

## Integration limits

- The app uses the original one-shot rendering API with a small cache. It does
  not integrate Grok's incremental checkpoint/freezing renderer yet, although
  that source is retained unchanged in the vendored crate.
- Only the fixed GrokNight palette is selected here. Automatic light/dark
  detection, theme switching and terminal capability probing from the full
  pager are not included.
- The adapter returns styled lines only. Original source maps, link hit
  targets, code-copy metadata and selection metadata are not connected to
  mouse actions or OSC-8 hyperlinks in this app.
- Image/video overlays, the full upstream pager, editor, agent runtime,
  network backend and Grok branding are not copied by this renderer port.
- Upstream crate tests remain in their source. Ordinary root `cargo test`
  runs the integration tests in `src/grok_markdown.rs`, not dependency-crate
  test suites. The latter can be run separately with each vendored crate's
  manifest if needed.

The previously cited `559751fdcec02d413e4c57c8832ab275e4f44980` was an old
checkout marker, not the verified source revision for this port. Attribution
for the files described here uses the actual checked-out and blob-verified
commit above.
