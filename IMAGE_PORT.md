# Image attachment integration and provenance

Reference: xai-org/grok-build fixed
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`, Apache-2.0.
No Grok authentication, agent runtime or full session manager is started.

## Retained original code

- `vendor/xai-grok-image/src/lib.rs`: original validation library, unchanged.
  Standalone manifest replaces workspace declarations. Its fixture-dependent
  upstream crate test suite is retained but not separately run; root tests call
  its validator through bounded staging.
- `src/terminal/image.rs`: original encoders, image fit and brand gating. Host
  imports its MIME sniffer and uses `dev-demo` as the test-support feature.
  Kitty conversion uses bounded in-memory decoding; macOS `sips` conversion and
  its temporary file are omitted. Original iTerm2 gating remains disabled.
- `src/terminal/overlay.rs`: original shared placement ownership, post-flush writes
  and cleanup. Host redacts encoded payloads from Debug.
- `src/render/image_overlay{.rs,/content.rs,/geometry.rs,/tests.rs}`: original popup
  and layout, with a wrapper reporting whether an overlay was painted. Preview
  approval is recorded by the host only after the terminal flush succeeds.
- `src/render/safe_buf.rs` and used line/byte-format helpers: original bounds-safe
  primitives, omitting unused larger-renderer dependencies. Dimming is the fixed
  RGB branch. `src/theme.rs` uses original GrokNight values, including paste
  background 17, foreground 200 and dim 65.
- `src/prompt_images.rs`: original image record, shared preparation/result state
  and display helper. Encoded bytes are redacted from Debug. Preparation remains
  off the interactive thread.
- `src/image_composer.rs` and `image_composer/image_state.rs`: extracted original
  atomic image chip insertion, undo stash/rebinding and cursor/hover/fresh-insert
  preview selection. The host omits the unverified Grok backend eight-pixel
  minimum, preserves the ten-image local cap, and drops only in-memory records on
  stash eviction. It never deletes explicitly selected source files.
- `src/file_picker/context.rs`: original @ context parser and its tests.
  `file_picker/dropdown.rs` retains the RGB dropdown paint algorithm. Host imports
  replace workspace-index state, shared scrollbar and configured glyphs. Embedded
  Reset/modal-theme branches are omitted. The dot index is bounded, nonrecursive
  and performs a filename substring filter in the current/explicitly typed
  directory: at most 1024 entries and 64 results. This is not the full original
  workspace fuzzy index, directory-drill UI or hidden/gitignored-file database.
- `src/image_clipboard.rs`: original `raster_try_chain` and combined
  `attachments_osascript`, preferring file URLs then PNGf/TIFF/JPEG. The host uses
  a private 0700 directory/0600 file, two-second deadline, child file-size limit
  and normal-exit cleanup. It does not import AppKit native reads, idle clipboard
  polling, global keyboard APIs, telemetry or unrelated platforms' providers.

Original encoder/popup/ownership tests are included. One `serial_test` attribute
is omitted because its test is the only extracted user of the global force-off
flag; capability overrides remain thread-local. These tests do not represent the
whole Grok pager test suite. No pixel-identical UI claim is made.

## Dot host adapters

`image_ingress.rs` stages only explicit local paths or paste input on the worker.
The live macOS paste route invokes the original combined probe; dev/tests use a
synthetic injected image and never read the system clipboard. Failed, stale or
canceled staging cannot recreate a cleared draft. Selection/paste never uploads.
The native macOS provider was compiled but not exercised against a real clipboard.

`main.rs` retains one verified dot/room, keyboard protocol and paste/focus barriers,
plus the existing durable journal. The immutable send plan includes only live
image records after reconciliation. Every selected image must have a flushed
local preview before submission. The pending marker is persisted before the first
upload. Unknown upload or message results retain the marker and draft, with no
automatic retry. Acceptance clears the exact marker and draft.

`image_transfer.rs` and `backend.rs` implement only the supplied verified official
front-end schema: multipart `file` POST at `/messaging/rooms/{room}/files`, returned
`id`, message attachments `{type:"file",file_id:...}`, and unchanged canonical
UUID request/idempotency fields. Uploads use the existing fixed chatgpt.com client,
with no redirects/proxy/retry. Real uploads/sends were not executed.

Received messages retain file IDs and render attachment chips using the original
chip styling. Clicking a chip requests room-scoped metadata, reads the supplied
`mime_type`, `name`, `download_url`, and optional image dimensions, then opens the
original popup. `image_download.rs` uses a separate anonymous client: no account
headers, cookies, proxies or redirects. It validates HTTPS/port/userinfo, rejects
private/local/reserved addresses, resolves once and pins public addresses before
connecting. No CDN domain name is guessed. The decoded bytes determine actual
image format/dimensions; conflicting metadata, non-images and oversized responses
are rejected. Viewing never uploads or sends. Esc cancels the view; stale results
cannot reopen it. OS DNS resolution precedes the HTTP timeout and has OS-defined
timing. Real download URLs have not been tested.

Local encoded bound: 20 MiB per image; dimensions: 8192 per side / 16 million
pixels; decoder allocation: 128 MiB. These are local preview policies. Only the
first decoded frame is checked/displayed by raster conversion. Clipboard staging
is private and cleaned on normal exit; abrupt process/OS failure cannot guarantee
cleanup or absence from OS crash dumps.

## Verification limits

Root tests and fake transports cover App input/worker/preview/submit and received
metadata/download/view. PTYs exercise the actual chat entry with a synthetic
binary clipboard, explicit PNG path and @ selector, plus the original keyboard,
focus, copy and incoming-history regressions. A separate synthetic image harness
checks Kitty encoding and metadata fallback. Neither a real account nor clipboard
is accessed, and no real file is uploaded. Previously denied Terminal window
access is not attempted. PTYs establish bytes/state restoration, not physical
pixel appearance in a real window. The full Grok runtime remains excluded.


## History-media and navigation correction

`upstream_media.rs` copies `scrollback/render.rs::media_open_button_label/col`
and the no-inline-graphics button line from `scrollback/blocks/tool/other.rs`.
The `[Open Image]` cell style is actual GrokNight `md_code` BLUE1 (58,149,171),
bold. Room history initially supplies IDs; names and dimensions arrive from
room-bound metadata. Pending/error text is a transport adaptation. No history
image-number placeholder or download-on-open-only block remains.
Render and click use the same stored row/column/width, including clipping/resize.

Grok `app/agent_view/media.rs::open_media_natively` routes history media clicks
to `link_opener::open_path`. Dot now follows that path through `native_media.rs`,
retaining original single-argv/detached-stdio behavior and adding bounded private
files for downloaded room images. Test/demo callers inject an opener; real OS
viewing, real downloads and screenshot appearance were not observed.
Draft previews use the historical viewport (as original `PromptWidget::draw`
receives `Some(layout.scrollback)`), and cannot cover status/prompt/footer.

The `▼` and three-column hit zone in `upstream_follow.rs` come from original
`draw_scroll_arrow`; its non-follow/content-below condition and goto-bottom
behavior are preserved, with placement adapted to the requested centered status
strip. The strip has one row and no surrounding blank gap rows. Structural page
anchoring follows the upstream first-mutation-wins and restore-before-input
principle. Dot retains ID + visual row and snapshot-based drawing; the complete
upstream logical-line reflow mapping is not imported.


## Complete applicable history image path (2026-11-05 correction)

`upstream_media::reserved_rows` directly uses the fixed upstream image branch of
`inline_media_ffmpeg.rs::inline_media_reserved_rows`: width minus two cells,
maximum height `(width / 2).clamp(4, 20)`, original `fit_image_to_cells`.
`ui.rs` ports the `scrollback/render.rs::inline_media_placements` virtual-row /
viewport intersection and top-crop calculation. `inline_history.rs` adapts the
`agent/render.rs` transmit-once, per-frame placement, offscreen deletion and
resize/modal cleanup lifecycle. All protocol encoding, alpha payload handling,
source-pixel cropping and image-format conversion use the existing upstream
`terminal/image.rs` and `prompt_images.rs` helpers. Image/button hits resolve
against the same immutable layout snapshot used for drawing.

Dot-specific boundary: a room-bound, read-only worker downloads only visible
media, at most two in flight. Cloned HTTP clients keep this work off the input
and page/send queue without rereading auth. The existing authenticated room
metadata + anonymous pinned-public-IP, no-redirect download policy is retained.
The private session cache supplies the same media object for inline pixels and
explicit native opening. A click during automatic loading joins its result;
cancel/focus/epoch guards still prevent late opening. Failed loads do not retry
each frame; an explicit click retries. Image-size layout completion preserves
message and attachment-relative anchors. Full-width styled rows prevent black
stripes inside user-message bands.

As in upstream, history inline overlays require the narrower placement capability:
Kitty/Ghostty/WezTerm; Warp, iTerm2 and Apple Terminal use the compact original
Open Image fallback and never reserve a blank large image. Draft overlay support
alone does not enable history inline graphics. Video/copy-image controls are not
included. No real account, clipboard, upload or send was used for validation.

Tests cover encoded PNG pixels (including alpha), wide/portrait fixtures,
multiple placements, top/bottom crop, clearing/resize/modal/exit, image/button
hit consistency, bounded loading, negative caching and canceled/late opening.
Actual Ghostty UI observation was denied by Computer Use for safety reasons.
Decoded payload proof and PTY bytes are explicitly not terminal screenshots or
proof of the terminal GPU/compositor result; real-window visual verification
remains blocked pending permission to access the isolated synthetic window.


### Review correction: bounded residency and transfer work

The history cache now admits at most 32 entries and 64 MiB of owned resident
payloads. Original bytes, separately converted PNG bytes and worker-preencoded
Kitty transmission bytes count toward the budget; shared Arc backing addresses
are counted once across entries. Owned path capacity and conservative metadata
cost also count. Rejected/replaced/evicted private directories are dropped.
Automatic admission protects currently visible entries. A capacity refusal or
explicitly evicted visible entry is suppressed until it leaves the viewport;
reentry can try again. An explicit Open may prioritize that item, while its
visible victims remain suppressed. This prevents same-screen eviction/download
loops and bounds the number of private cache directories.

Base64/Kitty packet construction runs on the image worker and is cached. Paint
copies at most 32 KiB of whole preencoded chunks per frame (at most 40 KiB with
bounded placement/deletion commands), with an 8 ms event-loop wakeup while
pending. Only one chunk stream is active; no other graphics command, including
prompt preview clearing, is interleaved before its final chunk. The same still-visible room/file/instance keeps upload progress through position,
crop, resize and asynchronous layout changes; the latest placement is applied
after completion. Existing placements are temporarily hidden before a long
exclusive stream to avoid stale pixel coordinates. Leaving view, modal, lost
backing data or exit closes a partial stream using an empty final chunk before
deleting/replacing it. Kitty's requirement is documented at
https://sw.kovidgoyal.net/kitty/graphics-protocol/ .

These are work/byte bounds, not a claim of nonblocking stdout or real-time input:
a terminal that stops reading can still exert backpressure. The existing prompt
image overlay path is unchanged. Synthetic PTY response measurements are in the
candidate report; real Ghostty GPU/window verification remains pending permission.

The 64 MiB limit is the retained session-cache payload/metadata budget, not whole
process peak RSS. Bounded in-flight downloads, transient decoding/encoding and
terminal-side graphics memory are separate; tests do not claim to cap those at
64 MiB. No real GUI validation has been granted.

## Superseding terminal-only viewer update

The earlier native-open sections describe superseded behavior. History Open Image
now uses render/image_viewer.rs, adapted from Grok agent_view/render.rs:3402–3567
and modal_window.rs close control at fixed 2bdd1d6. native_media.rs is only a
private cache; it contains no process creation or OS viewer command. Dot supplies
remote async attachment data in place of Grok local-path ImageViewer state.
Input image chips and preview remain the original modules, with paste theme
arguments matching PromptWidget. Pixel support is still capability-gated: Kitty
viewers on supported terminals, metadata fallback on unsupported terminals.
This update was compiled, not executed or visually validated.

## Prompt interaction completion

Initial composer/undo stash are empty. Paste inserts the original atomic chip
and sets the one-shot post-insert preview. Every following key/mouse operation
clears that one-shot, matching PromptWidget. Cursor-near-chip or hover selects
the original image overlay; typing after the chip moves the cursor away and
hides it, while clicking snaps the cursor onto the chip and reopens it. Mouse
Moved now reaches textarea hit testing, including HoverLeave outside input.

Human history attachment chips now prefix the message body (Dot's API has
separate file IDs, not Grok's local path placeholders). Wrapped text/cell hit
regions identify each room/file, so hovering shares the same prepared-image
overlay and moving off hides it. Loading remains asynchronous and bounded.
The explicit terminal viewer and nonhuman inline media paths remain intact.
The dedicated sent-chip identity/hover bridge is a Dot adaptation; no claim
is made that it was copied wholesale from a Grok state machine absent from
the pinned source. Source attribution and the image-only green accent change
are detailed in THIRD_PARTY_NOTICES.md.

All five supplied screenshots were viewed. Static inspection and release
compilation only; no program/test/PTY/GUI or terminal pixel validation.

## Fitted preview correction (supersedes 90% viewer routing)

Three additional screenshots were visually inspected: the status phase label,
Grok's centered prompt preview and Dot's incorrect large viewer. The latter
was the separate AgentView image_viewer renderer, not the shown PromptWidget
overlay path. Explicit history opening now calls the original image_overlay
renderer too; its fitted rectangle is reused by outside-click closing.
There is no close-cross or extra full-viewer horizontal matte on this path.
The image's own black pixels are not cropped or altered. Esc/q still closes.
Remote preview clones omit the metadata filename-as-path footer, while local
input-image paths retain Grok's original two-by-two metadata/path behavior.

The 30-second watchdog counted total phase/queue time, independent of actual
network requests. It could fail a serial multi-image submission despite each
request succeeding within its transport limit. That additional deadline and
its UI wording were removed; the individual HTTP 20s/connect 10s timeouts and
uncertain-send safeguards remain. This is static reasoning plus compilation,
not evidence of real interaction or live network outcomes.


### Preview event ownership follow-up

Complete image-specific state was compared against original
`views/prompt_widget/mod.rs` and its parent `app/mouse.rs` at 2bdd1d6.
The parent does not forward `MouseEventKind::Moved` to PromptWidget. Removing
Dot's extra forwarding preserves `post_insert_image_preview` during mere pointer
motion while retaining the original cursor/hover/fresh-insert selector, mouse
click/drag/release handling and focus gate. Explicit `set_cursor` retains the
original latch clear. Keyboard reset belongs to widget editing, not every global
application shortcut. History hover and the explicit viewer were not changed.
Build-only validation: no test, PTY, GUI or real preview interaction run.
