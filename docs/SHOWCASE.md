# Offline README showcase

Run from an interactive terminal (Ghostty recommended), sized to **128 columns × 40 rows**:

```sh
sh tools/showcase.sh
```

Or build and run manually:

```sh
cargo run --locked --features dev-demo -- --showcase
```

The script uses a separate `target-showcase` build. It does not replace the installed `dot-tui`, change Ghostty settings, or read credentials. `--showcase` is only available in development builds and cannot be combined with `--auth` or other scene fixtures.

The scene uses the real conversation, Markdown, timestamp, inline image, composer, and shortcut renderers. The messages are synthetic. The moonlit-lake artwork was generated for this showcase and selected by the user; no private conversation data is included. The image still uses the normal terminal graphics capability detection: Ghostty renders pixels; unsupported terminals show the ordinary image fallback.

The upper-right spinner uses the product’s existing **Reading messages** state, held as a simulated history refresh rather than an unfinished assistant generation. No showcase badge or demo-only status label is added to the UI. The answer is complete. Editing, scrolling, resizing, and opening the image work. Automatic polling is held so the initial composition remains stable. The prefilled input is an unsent draft; Enter sends only to the local fake backend and leaves the held refresh scene. Re-launch to reset the composition. Press **Ctrl+Q** to exit.

The showcase starts in English without reading or writing the saved language preference. Message times are fixed RFC 3339 values and use the normal local-time display. The initial Reading messages elapsed time is fixed at **1s** for screenshots; the spinner remains animated. Starting a new operation resumes normal elapsed-time tracking. The regular client is unaffected. The text-led scene includes an evening checklist and Rust snippet; the image is a compact attachment preview. The complete moonlit scene fits 128×40 without scrolling. This fixture limits the image preview to 54 columns while preserving its full 3:1 aspect ratio; the source image is not stretched or cropped. Smaller windows use normal scrolling.

## Customize the scene

Edit `src/showcase.rs`:

- `messages()`: conversation, Markdown, attachment IDs, and timestamps.
- `configure()`: unsent draft and simulated loading label.
- `assets/showcase/moonlit-night.png`: the embedded 2172×724 moonlit landscape, shown at its original aspect ratio.

Re-run the script after editing. No API calls or real conversation data are needed. This showcase does not capture or publish a screenshot automatically.
