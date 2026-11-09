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

## Composer and interactive scrollbar correction

`src/scrollbar.rs` retains production geometry, overflow rules, grab-zone,
ScrollMetrics inverse mapping and tui-scrollbar rendering from
`crates/codegen/xai-grok-pager-render/src/render/scrollbar.rs` at
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` (SpaceXAI, Apache-2.0).
Only theme access is adapted to the fixed GrokNight palette. Follow-mode dimming
uses the 0.4 blend from `pager/src/views/agent.rs`; drag integration follows
`pager/src/app/mouse.rs` (press, captured drag, release). The upstream rendering
bridge retains both ratatui versions and tui-scrollbar's own metrics.
`upstream_ui::user_lines` additionally preserves each wrapped line's inherited
style so muted composer placeholders stay muted. `src/editor.rs` is an original
small grapheme-aware text editor/layout; it does not claim to port Grok's full
textarea, selection, image chips or text input runtime.

## Original textarea, input dispatch and cursor presentation

`vendor/xai-ratatui-textarea/src/**` is copied byte-for-byte from
`crates/codegen/xai-ratatui-textarea/src/**` at the same Grok commit
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`. Its Apache-2.0 LICENSE, NOTICE,
original Cargo manifest and verified git-blob/SHA-256 manifest are retained.
Only its standalone Cargo manifest resolves workspace dependencies and omits
example-only dev dependencies. All original library tests remain present.

`src/upstream_input.rs` extracts the paste/key classification, coalescing and
fragment-merging functions from `pager/src/app/event_loop.rs`. The original AltGr
helper is imported from the textarea; Windows image-drop integration is omitted.
Dot control keys delimit batches so the original fragment merger cannot discard
send-confirmation controls. Bounded polling and dot modal dispatch are host adapters,
not the original full Grok event loop. No Grok auth/agent runtime is executed.

`src/upstream_cursor.rs` retains the original CursorState/CursorAction from
`pager-render/src/render/draw.rs`. Stock ratatui presentation replaces the custom
Grok writer. Prompt rendering uses the original TextArea StatefulWidgetRef and
cursor_pos_with_state flow from `pager/src/views/prompt_widget/mod.rs`, with the
original default focus/padding rules and fixed GrokNight theme colors. Cursor
OSC 12/112 policy follows `pager-render/src/theme/mod.rs`; shape is not forced.
The prior description of `src/editor.rs` is historical: that custom editor has
been removed rather than represented as original Grok input code.

The footer's five-column `  │  ` separator and Unicode display-width measurement
are adapted from Grok Build `crates/codegen/xai-grok-pager/src/views/shortcuts_bar.rs`,
commit `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` (xAI, Apache-2.0).
Dot reserves its review action before laying out hints and checks each complete
hint together with its separator to avoid partial hints or orphan separators.
The follow-up `src/upstream_shortcuts.rs` retains the original `paint_key_run`
and `paint_hint_keys` bodies and single-custom-display-key segmentation;
registry types are reduced to dot's single-key hints. The original row clear
and key/action/separator styles are adapted using actual GrokNight palette
fields from pager-render/src/theme/{groknight,tokyonight}.rs at the same commit.

`src/upstream_enter.rs` extracts PromptWidget::route_enter_with_newline_modifier
and backslash continuation from views/prompt_widget/mod.rs. File-search, Grok
runtime and Apple CoreGraphics polling are not included. `upstream_user_fold.rs`
retains the user block's foldability/display-mode bodies and adapts the plain
wrap/truncation path, same-entry click count and one-cell drag threshold from
scrollback/blocks/user.rs, app/agent_view/selection.rs and scrollback/text_selection.rs.
`upstream_text_selection.rs` is the original standalone appearance/text_selection.rs.
Focus routing follows app/mouse.rs and app/agent_view/panes.rs; outer scrollbar
geometry follows AgentViewLayout and actual ScrollbarConfig defaults. All are
from Grok Build fixed 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8, Apache-2.0.
Dot uses only the default Flash policy; it does not load remote Grok configuration.

`vendor/crossterm` is cached official crossterm 0.28.1 source, MIT license retained.
Five source files are modified to preserve CSI-u Enter provenance and expose a
bounded flags query using existing internal event filters. These are dot adapter
changes, not claims of unmodified upstream crossterm. The original source
provenance and precise modified-file list are recorded in KEYBOARD_ADAPTER.md.

`src/clipboard_bridge.rs` adapts ClipboardProvider/SystemClipboard from
pager-render/src/clipboard/mod.rs and the macOS plain text/spool/deadline paths
from xai-grok-shared/src/clipboard.rs at fixed 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
Its injected shared handle, one-read sanitized paste route and std-only unlinked
spool are dot host adaptations. OSC52, tmux and remote clipboard probes
are excluded. Raster reads are additionally extracted as described below. Focus dispatch follows app/event_loop.rs and agent_view/input.rs;
dot maps agent idle to its local operation state. Page ownership follows the
non-forced prompt route and scrollback route, omitting Grok-only panels/actions.
Latest-page polling/ID merge is a necessary dot GET-transport adapter; no Grok
agent runtime or websocket transport is claimed as reused.


The isolated image extraction checkpoint adds original Grok Build image validation,
Kitty/iTerm2 encoders, image overlay ownership, popup layout, image chip state,
and width/byte formatting from fixed 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8.
Apache-2.0 attribution and the precise original/host-adaptation boundaries are
recorded in IMAGE_PORT.md. No Grok auth, agent runtime or session backend is run.

The final image integration additionally retains the original @ context/parser,
RGB file dropdown and combined macOS file-URL/raster AppleScript at the same
fixed commit. Bounded directory indexing, background staging, dot upload schema,
received-attachment hit rows and anonymous pinned-address downloading are dot
host adapters. See IMAGE_PORT.md; no full upstream-session/runtime claim is made.


History media and follow indicators retain label/centering/button styles from
scrollback/render.rs and blocks/tool/other.rs, earlier native-open routing (now removed) from
app/agent_view/media.rs and pager-render/link_opener.rs, plus draw_scroll_arrow
from app/agent_view/render.rs, all at fixed 2bdd1d6 (Apache-2.0). Private room-file
staging, bounded caching, status-strip placement and snapshot anchor adapters are
dot host modifications; no full media/runtime or pixel-identical claim is made.

## Terminal-only history image viewer

`src/render/image_viewer.rs` adapts the image-viewer branch of
`crates/codegen/xai-grok-pager/src/app/agent_view/render.rs:3402–3567`
and the five-cell close control from `views/modal_window.rs::render_close_button`,
at the fixed Grok commit above (SpaceXAI, Apache-2.0). Preserved behavior includes
90% centered geometry, dimming, rounded frame, title/dimensions, static_centered
terminal image placement, and metadata fallback. Dot maps its async remote
attachment cache to the viewer instead of Grok's local-path ImageViewer state,
keeps the top status row outside the modal, and routes close events through
its event loop. Existing terminal overlay owner/post-flush cleanup is reused.
No external image opener remains. Prompt chips, hover/cursor selection and
image overlay remain the earlier extracted Grok modules; overlay colors now
use the original PromptWidget paste_bg/paste_fg/paste_dim arguments.

## Prompt attachment interaction and sent-chip adapter

The prompt adapter now retains the one-shot preview reset from Grok
`views/prompt_widget/mod.rs::handle_key` (1691) and `handle_mouse_inner`
(2242–2264), in addition to the existing `image_for_preview` (2603–2615).
Click snapping and element hit testing remain in the unchanged vendored
xai-ratatui-textarea `element_click_snap` / `handle_mouse`. Mouse Moved is
now routed to that state machine even when the pointer leaves the composer.
The original render/image_overlay.rs, geometry.rs and content.rs are shared
by prompt previews and sent-image hover previews.

Dot-specific adapter: remote history stores attachments separately from text,
so attachment-order chips are reconstructed and mapped to their immutable
message/file IDs and wrapped screen hit regions. This is not an unmodified
Grok sent-message hover state machine: the pinned source's UserPromptBlock
and media handler do not contain a dedicated sent-image-chip hover viewer.
The user-requested sent-chip hover uses the existing Grok preview module,
while clicks retain the terminal image viewer. Human attachments reserve no
permanent inline image/tool-action block. Dot's async anonymous downloader
and cache policies remain in effect. For the requested green image labels,
only image chip text uses GrokNight GREEN/accent_success (158,206,106); the
original dim brackets and chip structure are retained. This differs from
the pinned source's generic paste_fg image-chip text and is marked explicitly.

## Correct preview path and phase deadline removal

Ordinary history-image opening now shares the already extracted Grok
`xai-grok-pager-render/src/render/image_overlay.rs` and its geometry/content
modules with prompt/hover previews. It no longer routes through the separate
AgentView 90%-viewport viewer branch described above. The original fitted
image box/title/metadata and protocol preparation/placement are retained.
`image_overlay_rect` exposes the very same geometry for Dot's outside-click
hit test; no independent popup-size math is introduced. Dot remote file names
are metadata, not local pasted file paths, so only display clones clear that
field; the cached original name and real local-paste path behavior are retained.
The clicked chip's attachment number is preserved in async and cached previews.

The Dot-only 30-second UI phase watchdog and its two obsolete assertions were
removed. Per-request HTTP timeout, connection timeout, public-address pinning,
no-replay send behavior, journal locking and disconnected-worker detection
were unchanged in that historical revision. The journal lock is removed in
0.8.1 below. No tests or application execution were performed for that revision.

## Slash language menu and fold anchoring

`src/slash_dropdown.rs` copies the production renderer from Grok
`crates/codegen/xai-grok-pager/src/views/slash_dropdown.rs` at fixed
2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8 (SpaceXAI, Apache-2.0). Retained
algorithms include aligned label columns, Unicode description wrapping, fuzzy
spans, selected/hover row styling, wrapped row-to-item hit mapping and scroll
positioning. Dot adapts fixed RGB theme access, its existing scrollbar argument
types and prompt glyph; upstream embedded/minimal theme branches are omitted.
`src/slash.rs` supplies the compatible snapshot/row structures and original
wrapping Up/Down navigation, with a local provider for only `/language` and its
two argument choices. The Grok agent registry, skills and other commands are
not imported. Popup placement/divider/count now use the original chrome extraction below.

`src/ui.rs` fold anchoring adapts Grok scrollback/state/selection.rs
`capture_fold_anchor` / `rebuild_with_fold_anchor`: scroll-before plus selected
entry virtual-Y delta after rebuild, then viewport clamping. Dot resolves this
against its asynchronous timeline snapshot and minimally corrects visibility
when collapse removes the whole visible fragment. The inspected Grok
app/agent_view/selection.rs dispatcher explicitly calls scroll_to_entry_top
after a user-prompt double click; that navigation is intentionally omitted
to honor the user's requested stable viewport.

`src/i18n.rs` and `i18n_catalog.json` are Dot-owned UI-only localization and
private preference storage. Message and draft bodies do not enter that path.
Canonical image chips retain their existing round-trip identity.

## Original slash panel chrome and completion behavior

`src/dropdown_chrome.rs` extracts `render_dropdown_chrome`,
`dropdown_content_inset`, `dropdown_items_width` and `DropdownChrome` from
Grok `app/agent_view/mod.rs` at the fixed commit above. Fullscreen RGB branch
geometry, clipping, top/bottom hairlines, count hint and item inset are retained.
Dot passes its actual prompt left/right padding instead of `LayoutConfig`; the
unused embedded-theme branch is omitted. The fixed GrokNight panel-border
color follows original `Theme::panel_border_fg` (bg_highlight).

`src/slash.rs` now additionally retains the PromptWidget
`paint_slash_token_highlight` helper and adapts SlashController smart-prefix /
inline-ghost suffix construction. Source text and the real caret are not
mutated by hints. Keyboard navigation wraps, while wheel/page navigation
clamps, as in SlashController `move_selection` / `scroll_selection`. Tab and
mouse clicks complete text without persisting a language; Enter dispatches
the local language action. The input loop repaints menu geometry between
events to preserve the original `InputOutcome::Changed` contract. Native-name
argument descriptions and active suffix follow `commands/theme.rs::suggest_args`.

The only provider remains Dot `/language` with two enum values. There is no
LanguageCommand in the inspected Grok source; the setting action, bilingual
labels and persistence remain the necessary Dot adaptation. No extra command,
modal, dependency, model action or account behavior is introduced.


The macOS clipboard follow-up extracts the native AppKit pasteboard helpers,
raster type classification/priority and process-wide pasteboard mutex from
`crates/codegen/xai-grok-shared/src/clipboard.rs` into `src/clipboard_native.rs`.
It retains the original objc2 0.6.3 runtime messaging and lazy AppKit load.
`src/clipboard_paste.rs` copies the pure paste payload/text comparison, probe
routing and metadata gate from pager-render's `src/clipboard/mod.rs`.
All use fixed upstream 2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8, Apache-2.0.
The existing `src/image_clipboard.rs` preserves the original AppleScript raster
and file-URL generators and now prefers the original native image read.
Dot adaptations: attachment-budget check before native allocation; existing
private temp files and helper deadline; a bounded serial staging channel instead
of Grok's effect runtime; draft-epoch completion fence instead of session target;
Grok's FocusPrompt+forward, synchronous bracketed-text insertion with deferred
image probe, text/clipboard-generation matching and top-right error feedback
are mapped into the existing Dot event loop. No Grok agent/backend runtime,
telemetry, terminal clipboard synchronization or focus-driven clipboard tip is
claimed to be copied. Existing image insertion/preview modules remain in use.


## Prompt image preview event ownership

The preview-state follow-up checks the complete image-specific state and event
path at fixed 2bdd1d6: `views/prompt_widget/mod.rs` fields, `handle_key`,
`handle_mouse_inner`, `handle_paste`, `set_cursor`, image insertion/sync/clear,
`image_element_near_cursor`, `image_for_preview` and focused rendering; plus
`app/mouse.rs` pane dispatch. The original preview selector remains
cursor-on-chip/right-edge, then hover, then just-inserted image at its saved
cursor. Image identity and orphan/undo reconciliation remain unchanged.
Dot now follows the original parent mouse dispatcher: plain `Moved` updates
pane/link/button hover without calling PromptWidget's interaction handler.
The previous extra forwarding cleared the just-inserted latch on every pointer
motion. Prompt clicks/releases/drags still use the original textarea handler;
clicking another pane hides the overlay through the original focus gate.
The original `set_cursor` transition is retained explicitly rather than falling
through DerefMut. Editing keys, text paste and the extracted Enter route clear
the one-shot at their widget boundary; unrelated app-level shortcuts do not.
Dot's independent history-image hover and explicit image viewer are preserved.
No new preview persistence rule, timer or heuristic is introduced.


## Minimal distribution tooling

`install.sh` follows the platform/Rosetta detection (original lines 173–198) and
staged atomic move (285/308) of Grok Build's
`crates/codegen/xai-grok-pager/scripts/install.sh` at fixed
2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8 (SpaceXAI, Apache-2.0).
Dot omits Grok auth/config/profile/alias/completion handling and adds its own
fixed GitHub release, SHA256 verification, stream extraction and login guidance.
`tools/build_release.py` and `tools/collect_licenses.py` are Dot build adapters.
The three previously absent theme/Swift license files are restored byte-for-byte
from dot-tui commit 625134252dbfb96d83b274c5dbf0f8d5cb224351.
Three crates omit their workspace license from the downloaded crate: objc2 0.6.3,
objc2-encode 4.1.0 and tui-scrollbar 0.2.8. Exact-revision MIT texts are retained
under licenses/crates, with source URL/blob in PROVENANCE.json. tui-scrollbar's
MIT alternative is selected; the declared MIT OR Apache-2.0 expression remains
in the inventory. The release builder bundles all selected normal/build package
license/notice files, together with these extracted-code and asset notices.


## Embedded syntax/theme notices for distribution

The actual Markdown build uses `two_face::syntax::extra_newlines()` to build the
embedded syntax dump, then adds the locally retained Swift definition. two-face
0.4.5 explicitly distinguishes its crate MIT/Apache texts from the independent
licenses of embedded assets. Its complete `generated/acknowledgements_full.md`
is retained verbatim at source commit
4f766bcb4499f6340e8d6dd715ab64f52c6e8ca4 under
`licenses/assets/two-face-0.4.5/ACKNOWLEDGEMENTS.md`. Both Syntaxes and Themes
sections are preserved for conservative coverage; this is not a claim that every
listed theme is rendered by dot-tui. The original README's bat NOTICE is retained
separately. This supplement is always bundled, even though two-face already has
ordinary crate license files. Exact sources/blob hashes and local file hashes
are in `licenses/assets/PROVENANCE.json`.

Markdown loads its own generated syntax dump and Tokyo Night theme, rather than
calling syntect's default syntax/theme loaders. Syntect's MIT text, two-face's
own MIT/Apache texts, the full upstream asset document, the Grok/Tokyo Night
notices and the separate modified Swift attribution are all retained. Root
crate license collection is not treated as sufficient evidence for embedded
assets. Future dependency/asset upgrades require matching supplemental notices.

## Send editing and diagnostics adaptation (0.8.1)

Reference: Grok Build `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`, Apache-2.0:
`crates/codegen/xai-grok-pager/src/app/agent_view/prompt.rs` (widget editing),
`app/dispatch/prompt.rs` (consumed input/history and image stash), and
`app/effects/mod.rs` (send start/done, elapsed time, result and request identity).
The existing extracted PromptWidget/TextArea are reused unchanged. Dot-specific
glue keeps consumed image/chip state in session history and maps its HTTP upload
and message stages to redacted diagnostics. This is not the Grok ACP runtime or
an exact copy of its agent/session/queue implementation.

The 0.8.1 diagnostics entry uses the original `ClientLogEntry` field layout
from `crates/codegen/xai-grok-telemetry/src/logs/unified_log.rs` and
`make_entry`/`now_ts` bodies from `xai-grok-pager/src/unified_log.rs` at
2bdd1d6 (Apache-2.0). Dot substitutes its version and an owner-only local
sink; ACP forwarding, session IDs, and external telemetry are not included.
Only allowlisted HTTP stage/error context is recorded.

## Prompt viewport and wheel parity

`src/upstream_ui.rs` now uses the original `area.height / 2` composer limit
from `app/agent_view/render.rs:804–811` at fixed Grok commit
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`, replacing Dot's eight-row cap.
The existing `PromptWidget::desired_height` extraction retains its measured
text rows, padding and clamp. Dot still omits agent-only optional panels.
The mouse-wheel branch in `src/main.rs` follows
`app/agent_view/panes.rs:1245–1299`: hit-test the prompt, let the original
textarea consume scrolling, and fall back to history when it cannot scroll.
`ImageComposer::handle_mouse` already returns the vendored `MouseAction`, so
Dot tests `Scrolled` directly instead of recreating `handle_mouse_scroll`.
The original vendored TextArea owns scrolling, clamping, cursor-independent
viewport overrides, and resuming cursor-follow on editing. No new scrolling
algorithm is introduced. Dot's event-loop and modal/read-only gates remain.

Code-fence rendering continues to use the unchanged original Markdown crates
and syntax assets recorded by `vendor/UPSTREAM_MANIFEST.json`. Regression
checks cover hidden fences, highlighting, wrapping, and preserving original
spans/background through Dot's timestamp/gutter wrapper. These checks do not
claim parity with Grok's agent/tool-specific code viewers or all interactive
code-block actions, which are not part of this message-rendering adapter.

## Prompt initialization and editing-key follow-up

The composer now copies the missing scrollbar initialization from pinned
Grok `views/prompt_widget/mod.rs:711–716`: background `bg_base`, thumb
`selection_border`, and one-column padding. `App::new` constructs the real
ImageComposer rather than replacing its initialized textarea with a generic
TextArea. The fixed GrokNight selection-border color is `(60,60,65)`.
The Ghostty-only Cmd+A gate and selection/caret operations follow the same
file's `cmd_a_select_all_supported` and `handle_key:1872–1881`. Other terminal
brands keep their own Cmd+A behavior. Ctrl+A remains the unchanged original
textarea line-start command. Dot's conflicting Ctrl+H history and Ctrl+U
whole-draft clear shortcuts were removed so the original backspace and
kill-to-line-start commands (including repeated key events) can run.
Dot still supplies terminal-brand detection and its focus/modal routing.
The installed Grok reports `1.0.77 (50ca579fa996)`, but that revision is not
available in the public source repository (GitHub returned ref-not-found).
These source comparisons use the documented public pinned revision; they do
not claim byte-identical parity with that unavailable installed revision.
