# UI port and verification boundaries — 0.6

## What changed from the rejected 0.5 screenshot

- Chat: real Markdown parsing and styles replace literal **markers**; original
  styled word wrapping replaces character-by-character wrapping. User bands use
  the original prefix/continuation and padding logic. Real local message times
  occupy the original 10-cell reserved first-line gutter.
- Prompt: original chrome painting and desired-height formula replace a fixed
  5-row box. Empty/single-line input is 3 rows, then grows with wrapped text.
- Layout: original AgentViewLayout constraint ordering drives header, timeline,
  turn-status, prompt and shortcut rows, with outer padding and gaps.
- Footer/status: plain subdued click targets replace filled button backgrounds;
  repeated page-count status is removed. Dot-specific truthful labels remain.
- Scroll: original stream classification/acceleration/fractional carry replaces
  three-lines-per-raw-event scrolling; input timing and deferred flush matter.

## Exact code provenance

See THIRD_PARTY_NOTICES.md and vendor/UPSTREAM_MANIFEST.json. This is a bounded
port of actual source, not the full Grok application. Markdown parser/rendering
source and assets are preserved; layout, terminal profile selection and event
integration are adapted to a single existing dot conversation.

## Still not identical

- Dot does not expose Grok coding-agent task/tool/diff blocks through this text
  adapter. No fake tool state, token counts or model identity are displayed.
- Timestamp hover expansion, timeline rail, drag selection/copy, image previews,
  link-click actions and editable cursor positioning have not been ported.
- Terminal profile auto-detection is bounded to environment information; the
  full upstream capability-probing/settings runtime is not included.
- Input confirmation remains dot-specific to prevent unintended real tasks.
- OS terminal font, font size, line height and display scaling belong to the host
  terminal and cannot be matched merely by changing Rust layout code.

## Required visual acceptance

Build the developer executable separately and run --demo without any real login.
Match both terminal windows' columns/rows, font family/size and display scale.
Capture actual windows, not a reconstructed image. Compare padding, line wrapping,
message time alignment, prompt height, status/footer gaps and mouse interactions.
Different message contents do not support pixel-level similarity claims.

Mac ran both production and developer configurations with 96 tests passing in
a color-capable environment. A NO_COLOR-dependent test assertion was subsequently
corrected without forcing color; that environment still needs its regression run.

Terminal access for actual screenshots was explicitly denied. No alternative
capture was attempted. Same-grid visual acceptance remains blocked, not completed.
Passing unit or buffer tests is not visual equivalence or real-account verification.

## 0.7 composer and scrollbar correction

The empty composer keeps a muted placeholder without adding it to the draft.
A native terminal caret follows a grapheme-aware editor and the same display-cell
layout used for painting, including wrapped Chinese/emoji, newlines and long input.
Its visual viewport follows the caret rather than always showing the final rows.
The transcript now reserves the original one-column gap and one-column scrollbar
only on overflow, remeasures at the reduced width, and shares those metrics across
rendering, wheel bounds, click and drag. Follow mode dims the thumb; mouse-up,
keyboard/modal ownership, paste and viewport changes cancel stale captures.
This is the vertical scroll indicator; the separate turn timeline/bookmark rail
and other unported selection/tool UI remain outside this change.
