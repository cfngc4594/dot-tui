# dot-tui: ChatGPT dot terminal client

**dot-tui** is an open-source terminal user interface (TUI) for chatting with your ChatGPT dot. It is built with Rust and [Ratatui](https://ratatui.rs/).

- Read conversation history, send messages, and attach images from your terminal.
- Render replies as Markdown and use `--read-only` to browse without composing.

## Install

The [latest release](https://github.com/cfngc4594/dot-tui/releases/latest) provides a prebuilt macOS arm64 binary for Apple Silicon Macs.

```sh
curl -fsSL https://github.com/cfngc4594/dot-tui/releases/latest/download/install.sh | bash
```

## Run

Sign in to Codex with your ChatGPT account and make sure your dot already has a conversation. Then launch:

```sh
dot-tui
```

For a read-only session:

```sh
dot-tui --read-only
```

dot-tui uses your existing Codex login and a non-public ChatGPT backend. This integration is experimental. See [security and limitations](SECURITY.md) for authentication details, terminal requirements, and live-account validation status.

## License

[Apache 2.0](LICENSE)
