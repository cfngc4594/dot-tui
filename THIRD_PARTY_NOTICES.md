# Third-party source and references

## Grok Build

https://github.com/xai-org/grok-build
Inspected source commit: 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
Copyright 2023–2026 SpaceXAI. Apache License 2.0.

`src/sanitize.rs` adapts `strip_control_chars` from
`crates/codegen/xai-grok-pager/src/views/goal_detail.rs`. Modifications are marked
in the file (bidi controls, rendered-length limit). License copy is in
`licenses/Grok-Build-APACHE-2.0.txt` and this project is Apache-2.0.

Architecture reference (not copied wholesale): `xai-grok-pager` Cargo dependencies,
`app/agent_view/key_owner.rs`, `app/terminal_restore.rs` and event loop separation.
The compact UI additionally references `views/welcome/prompt.rs`,
`views/welcome/top_bar.rs` and explicit mouse hit-region handling. `src/ui.rs` is
an original implementation informed by those patterns and the user screenshot.
No Grok model, agent runtime or logos are shipped. The original Markdown renderer
and its syntax/theme assets are now vendored as described below.

## chatgpt-dot-cli

https://gitlab.com/aa22396584/chatgpt-dot-cli
Inspected protocol commit: 919700cf15a475306233139b72c3a3853d3fdd38.
Copyright (c) 2026 ImL1s. MIT.

Rust API adapter is independently implemented from the protocol and the earlier
Python prototype. The full MIT notice is retained in licenses/chatgpt-dot-cli-MIT.txt.

Dependencies are fetched from crates.io under their respective licenses.
Cargo.lock pins the tested resolution. They are not vendored into this source package.

## Actual UI/renderer reuse in 0.6

- `vendor/xai-grok-markdown` and `vendor/xai-grok-markdown-core`: original parser,
  renderer, syntax engine and assets; Rust sources/assets are byte-identical to
  upstream. Standalone Cargo manifests replace workspace dependency declarations.
- `src/grok_markdown/{wrapping,line_utils,md_style}.rs`: original styled wrapping,
  selected line helpers and style mapping; adapter differences are marked.
- `src/upstream_ui.rs`: adapted AgentViewLayout constraints, PromptWidget sizing
  and border painting, UserPromptBlock prefixes and EntryRenderer timestamp gutter;
  original braille spinner frames and turn-status cadence.
- `src/grok_scroll.rs`: adapted original mouse/trackpad stream classification,
  acceleration, fractional accumulation, reversal and coasting algorithms.
- `vendor/UPSTREAM_MANIFEST.json` records verified source git blobs;
  `vendor/README.grok.md` describes exact preserved sources and modifications.

The former SOURCE_REV note was not a verified git revision. This release records
the actual checked-out commit shown above rather than relying on that note.
All retained Grok portions are Copyright 2023–2026 SpaceXAI, Apache-2.0.

## Bundled syntax/theme asset lineage

The original Grok Build THIRD-PARTY-NOTICES entry for Tokyo Night is retained in
`licenses/Tokyo-Night-Upstream-Notice.txt`. It attributes the nvim palette to
Folke Lemaitre and contributors under Apache-2.0 and identifies the original
Enkia VS Code palette lineage. The Enkia MIT license is additionally retained
in `licenses/Tokyo-Night-Enkia-MIT.txt`. These notices apply to the bundled
`tokyo-night.tmTheme` and the reused palette colors.

The bundled `Swift.sublime-syntax` identifies its origin as
`quiqueg/Swift-Sublime-Package`, via bat v0.25.0, with upstream Grok modifications
marked in its header. Copyright (c) 2014 Matthew Alexander. Its original MIT
license is retained in `licenses/Swift-Sublime-MIT.txt`.
