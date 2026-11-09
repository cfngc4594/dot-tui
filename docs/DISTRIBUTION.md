# Distribution tooling

Release engineering uses Rust under `tools/xtask`, with an independent manifest
and lockfile. Its dependencies do not enter the app's bundled runtime licenses.
Packaged users need neither Python nor Rust. Builders need Rust/Cargo 1.89+ and
the native platform toolchain (Apple SDK, Linux C toolchain, or MSVC Build Tools).

## Native targets

| Package target | CI host | Notes |
| --- | --- | --- |
| `aarch64-apple-darwin` | `macos-15` | Apple Silicon; macOS 11+ declared |
| `x86_64-apple-darwin` | `macos-15-intel` | Intel; macOS 11+ declared |
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | glibc Linux, not musl/Alpine |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | glibc Linux, not musl/Alpine |
| `x86_64-pc-windows-msvc` | `windows-2022` | Windows x64; PowerShell or Git Bash |

A matrix definition is not evidence that all platforms have built or run.
Check the relevant CI result before publishing a target. The builder checks that
its target matches the actual Rust host; it does not silently cross-compile and
claim native validation. Linux bundles are built on Ubuntu 24.04 and are not
promised to run on older glibc distributions. The Linux installer requires
glibc 2.39 or newer, and rejects musl or an unidentifiable libc before downloading.
WSL installs the Linux binary.
Windows ARM64 is not currently packaged.

## Prepare a bundle

From the project root:

```sh
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- build-release
# Or require an explicit matching native target:
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- \
  build-release --target aarch64-apple-darwin
# Dependencies must already be cached for offline builds:
cargo run --manifest-path tools/xtask/Cargo.toml --locked --offline -- \
  build-release --offline --output /absolute/new/output/directory
```

The app is built with `--release --locked --target <TARGET>`, without demo
features. This command never runs the app or installer and never publishes a
release. Output must not exist or sit inside a publication source directory;
use `dist` or an external directory. Each target bundle includes:

- `dot-tui-v<VERSION>-<TARGET>.tar.gz`: executable (`dot-tui.exe` on Windows),
  `VERSION`, README and license notices.
- `install.sh` and `install.ps1`: version-matched installers.
- `SOURCE_FILES.txt`: explicit source boundary excluding caches.
- `release-manifest.json`: target, toolchain, source/asset hashes and licenses.
- `SHA256SUMS`: prepared asset checksums.

Tar entries are flat, sorted, fixed-mtime, with fixed ownership/modes. Identical
archive inputs package deterministically; reproducible compiler output across
hosts is not promised. Paths are remapped to generic build roots and binaries
are scanned for private input paths. Source inventory rejects symlinks/newline
names and excludes `target`, `target-*`, `dist`, `.git`, caches and logs.

macOS packages retain architecture and strict code-signature checks. The current
signature is ad-hoc, not Developer ID or notarization. Linux and Windows packages
are not publisher-signed. SHA256 checks integrity against release metadata, not
an independent publisher identity. No helper disables Gatekeeper, execution
policy, antivirus or quarantine protections.

## Installers and PATH

The shell installer selects Darwin/Linux or native Windows (Git Bash/MSYS),
including the official Grok-style Rosetta architecture correction. PowerShell
uses `install.ps1`. Installers download only versioned GitHub release assets,
verify SHA256, version and architecture, and stage files before replacement.
Before replacing an existing installation, they run only the verified staged
binary with `--version` and require the expected version. Missing runtime
libraries or loader failures therefore leave the existing binary untouched.
They never start an interactive session, read login credentials, or install Rust. No user shell profile or
persistent PATH is changed automatically; follow the printed PATH instruction
if the destination is not already on PATH. A piped child shell cannot change
its parent's PATH. Windows may refuse replacing an executable while it is open;
close that app and retry, rather than killing it or weakening file protections.

Reference inspected: <https://x.ai/cli/install.sh> (platform detection, Rosetta,
staging and PATH behavior). That upstream currently also edits shell profiles
and manages other CLI aliases; dot-tui deliberately does neither. The public
`xai-org/grok-build` installer/CI paths returned 404 during this review, so these
are not represented as copied or verified upstream CI implementations.

## License collection

```sh
cargo metadata --locked --format-version=1 --filter-platform aarch64-apple-darwin > metadata.json
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- \
  collect-licenses metadata.json THIRD_PARTY_LICENSES.txt --inventory inventory.json
```

Use the target being shipped, not the builder's unrelated host graph. Only
normal/build dependencies reachable from the app root are collected. Texts are
verbatim with ordinary newline normalization. Missing texts fail closed; only
exact-version overrides under `licenses/crates` are accepted. Resolved paths
must remain inside each package. Embedded assets additionally require versioned
provenance and matching hashes under `licenses/assets`.

## Local replacement and CI

```sh
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- \
  publish-release /path/to/verified/dot-tui /absolute/destination/dot-tui
cargo test --manifest-path tools/xtask/Cargo.toml --locked
```

Despite its name, `publish-release` replaces only a local file, not a GitHub
release. It stages a separate file in the destination directory, syncs and checks
hashes (plus macOS signature), then renames without truncating an existing binary.
Symlink destinations are refused; failures are reported without launching code.
Use temporary destinations for validation, never a user's running installation.

The manual-only workflow has read-only repository permissions and retains
per-target Actions artifacts. It runs native app/tool unit tests and isolated
installer tests using the local built archive, followed by `--version`; no user
installation or login is touched. It does not publish. Before a combined GitHub
release, review every target artifact, use unique archive names, retain each
per-target manifest, and combine checksum entries for all archives plus the
identical installers. Do not overwrite one target's manifest with another or
publish a checksum list containing only the final target. Installer URLs assume
one combined `SHA256SUMS` at the version's release root. Inspect actual build
results, notices and source boundary before publication.

### Terminal and authentication limits

Git Bash installation does not imply MinTTY graphics/console compatibility.
A native Windows console (for example Windows Terminal running PowerShell or
Git Bash) is preferred. In MinTTY, `winpty dot-tui` may be required; this path
needs separate interactive validation. The installer never changes PowerShell
ExecutionPolicy, so policies that block `.ps1` files may prevent the Git Bash
handoff. Follow your existing approved PowerShell policy rather than disabling it.

Each computer needs its own existing official Codex ChatGPT login file. The
client uses `CODEX_HOME/auth.json`, or the platform home `.codex/auth.json`;
`--auth` selects another existing file. API keys and keychain-only logins are
not converted by the installer. No credentials are transferred during builds.
Linux system clipboard requires a working X11/Wayland session; without one,
terminal bracketed text paste and `@` file selection remain available. Image
preview protocols depend on the terminal/multiplexer, not the installation shell.

Windows installs default to `%USERPROFILE%\.local\bin`. Custom destinations must remain inside the current user profile. The installer rejects reparse points, untrusted owners, and modification/delete/permission grants to principals other than the current user, SYSTEM, or Administrators; it never changes an existing ACL. Existing executable and notice files receive the same checks before replacement.
