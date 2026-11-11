# dot-tui: ChatGPT dot terminal client

**dot-tui** is an open-source terminal user interface (TUI) for chatting with your ChatGPT dot. It is built with Rust and [Ratatui](https://ratatui.rs/).

- Read conversation history, send messages, and attach images from your terminal.
- Render replies as Markdown and use `--read-only` to browse without composing.

![dot-tui terminal interface](docs/assets/showcase.png)

## Install

Native packages target macOS (Apple Silicon/Intel), Linux (arm64/x64, glibc 2.39+), and Windows x64.

macOS, Linux, or Git Bash:

```sh
curl -fsSL https://github.com/cfngc4594/dot-tui/releases/latest/download/install.sh | bash
```

Windows PowerShell:

```powershell
irm https://github.com/cfngc4594/dot-tui/releases/latest/download/install.ps1 | iex
```

Follow the installer’s PATH instructions if needed. See [platform and terminal limits](docs/DISTRIBUTION.md#terminal-and-authentication-limits), including MinTTY and PowerShell policy requirements.

## Run

Sign in to Codex with your ChatGPT account and make sure your dot already has a conversation. Then launch:

```sh
dot-tui
```

For a read-only session:

```sh
dot-tui --read-only
```

For a trusted local proxy, run `HTTPS_PROXY=http://127.0.0.1:7897 dot-tui` (replace the example port); use `--no-proxy` for direct access. See [proxy details](docs/PROXY.md).

dot-tui uses your existing Codex login and a non-public ChatGPT backend. This integration is experimental. See [security and limitations](SECURITY.md) for authentication details, terminal requirements, and live-account validation status.

## License

[Apache 2.0](LICENSE)
