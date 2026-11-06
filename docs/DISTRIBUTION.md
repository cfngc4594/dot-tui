# Distribution preparation: 0.8.0

This is prepared locally, not a published release. The public repository is
https://github.com/cfngc4594/dot-tui. At inspection its main branch was
`625134252dbfb96d83b274c5dbf0f8d5cb224351` (2026-11-04), predating the current
image, clipboard, language and scroll fixes. The local working copy is not a Git
checkout. No push, tag, Release, upload or permission change was performed.
Do not overwrite newer local files with that old main branch when synchronizing.

## Scope

Only `aarch64-apple-darwin` is packaged. Apple Silicon macOS 11+ is declared by
the executable; actual compatibility with older macOS/terminals is not validated.
Rosetta shells on Apple Silicon are detected. Intel Macs, Linux and Windows are
rejected by this installer rather than receiving the wrong executable. Linux's
image clipboard and Windows's Unix filesystem/authentication assumptions remain
separate platform work.

The binary does not require Python or Rust on the user's computer. Python 3.11+,
Rust/Cargo and Apple's C compiler/SDK are release-builder dependencies. The
declared Rust floor is 1.89 because the locked uuid dependency requires it; local
build evidence records the actual installed toolchain. The prepared CI definition
pins 1.89.0, uses the documented arm64 `macos-15` runner and has not been executed.
Runner source: https://docs.github.com/en/actions/reference/runners/github-hosted-runners

## Prepare a bundle

On native Apple Silicon macOS, from the project root:

```sh
python3 tools/build_release.py
# With all locked dependencies already cached:
python3 tools/build_release.py --offline --output /absolute/new/output/directory
```

The build is production `cargo build --release --locked --target aarch64-apple-darwin` with no demo feature.
It does not run the executable, installer or tests. The output directory must
not exist. The builder verifies arm64 and the embedded code signature, collects
verbatim dependency licenses using the non-dev target graph, then prepares:

- `dot-tui-v0.8.0-aarch64-apple-darwin.tar.gz`: executable, version and notices.
- `install.sh`: fixed-version installer.
- `SHA256SUMS`: SHA256 of the archive and installer.
- `release-manifest.json`: build toolchain, source hashes and license inventory.
- `SOURCE_FILES.txt`: explicit source publication paths; build caches are excluded.

The archive metadata is fixed, but a byte-for-byte reproducible compilation is
not claimed. Rust and native compiler file paths are remapped to generic `/build`
roots. Packaging refuses a binary that still contains the private build prefixes
or a user-home path. Source inventory covers root manifests/docs, source, vendor,
licenses, tools, documentation and workflow definitions; `target`, `target-*`,
Python caches and log files are excluded. Do not push the whole task directory.
The currently built executable is ad-hoc signed, without Developer
ID identity or a notarization pipeline. SHA256 checks download integrity; it is
not an independent publisher signature. No installer step changes Gatekeeper,
removes quarantine attributes or starts the executable.

## Future one-line installation

**This command becomes usable only after the reviewed v0.8.0 assets are actually
published at the versioned GitHub Release URL. They have not been published here.**

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://github.com/cfngc4594/dot-tui/releases/download/v0.8.0/install.sh | sh
```

Users may instead download/inspect `install.sh` and run `sh install.sh 0.8.0`.
It downloads only from that version's public release, verifies the archive SHA,
version, architecture and embedded signature, then installs a new inode at
`~/.local/bin/dot-tui`. `DOT_TUI_INSTALL_DIR` can select another absolute user
directory. Existing executable symlinks are refused. The installer requires no
sudo, does not create `grok`/`agent` aliases and does not edit shell profiles or
Grok files. License texts are retained under
`~/.local/share/dot-tui/releases/v0.8.0/`. A different archive cannot silently
reuse an installed version. The executable replacement is atomic; an unsuccessful
download/verification leaves the previous binary intact. Completed license
metadata may remain if a later binary installation fails.

Installation does not sign in. Before running, use official Codex's ChatGPT
login and initialize an existing dot conversation in the official app. dot-tui
reads `--auth`, `$CODEX_HOME/auth.json` or `~/.codex/auth.json`; it does not ask
for tokens, support API-key login or refresh expired credentials. Login files
must be owned by the user and private. The installer never reads them. The
application needs a real interactive terminal and network access to the service.
Terminals must forward paste events, and picture display depends on their
supported graphics protocol. See SECURITY.md for existing service limitations.

## Provenance and pending publication

Grok Build's installer at fixed
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`,
`crates/codegen/xai-grok-pager/scripts/install.sh`, supplies the platform/Rosetta
and stage-then-rename reference. Dot retains only those required branches;
Grok deployment auth, parallel download machinery, aliases, shell completions,
configuration migration and profile rewriting are not included. Dot adds pinned
GitHub assets, archive checksums, stream-only extraction and its login guidance.

The three theme/Swift license files were restored verbatim from the inspected
dot-tui commit. Supplemental objc2/objc2-encode/tui-scrollbar license texts come
from their exact crate VCS revisions; sources are in
`licenses/crates/PROVENANCE.json`. The builder fails if a selected package has
no license text, rather than synthesizing one from an SPDX label. Registry crate
license/notice files and extracted-code/asset notices are bundled together.
Crate license counts alone are not a complete embedded-asset inventory:
the full two-face 0.4.5 syntax/theme acknowledgements and bat NOTICE are retained
at their exact crate VCS revision under `licenses/assets/`, independently of
the crate's own MIT/Apache texts. `licenses/assets/PROVENANCE.json` records scope,
source and hashes; missing or changed supplements stop packaging. The full
upstream document is conservatively retained, including its theme notices.

Before an actual publication: reconcile the local source with repository main,
review the complete prepared diff and notices, run the manually triggered build
if desired, and approve the precise version/assets. The workflow has read-only
contents permission and manual triggering only; it can retain Actions artifacts
when deliberately run, but cannot create or publish a GitHub Release. No runtime
or installer validation is claimed for this preparation stage.
