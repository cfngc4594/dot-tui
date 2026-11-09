# Distribution tooling

Release engineering is implemented in Rust under `tools/xtask`. It has an
independent manifest and lockfile, so tool dependencies do not enter the app's
runtime graph or bundled runtime-license inventory. Users of the packaged binary
need neither Python nor Rust. Release builders need Rust/Cargo 1.89+, native
Apple Silicon macOS and Apple's command-line tools/SDK.

## Prepare a bundle

From the project root:

```sh
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- build-release
# Both tool and app dependencies must already be cached for offline use:
cargo run --manifest-path tools/xtask/Cargo.toml --locked --offline -- \
  build-release --offline --output /absolute/new/output/directory
```

This builds the app with `--release --locked --target aarch64-apple-darwin`,
without demo features. It does not run the app, installer or tests, and never
uploads or creates a GitHub release. The output directory must not exist or sit inside a publication source directory;
use `dist` or a directory outside the repository.
It checks the installer version, native arm64 architecture and embedded signature,
then prepares:

- `dot-tui-v<VERSION>-aarch64-apple-darwin.tar.gz`: executable, VERSION and notices.
- `install.sh`: version-matched installer.
- `SOURCE_FILES.txt`: explicit source publication boundary, excluding build caches.
- `release-manifest.json`: toolchain, source/asset hashes and license inventory.
- `SHA256SUMS`: archive, installer, source list and manifest checksums.

Archive entries are flat, sorted, fixed-mtime, and use fixed ownership/modes.
Repeated packaging of identical inputs is deterministic; byte-identical Rust
compilation or equivalence with the retired Python compressor is not claimed.
Rust and native compiler file paths are remapped to generic `/build` roots.
The builder refuses executables containing private input paths or user-home paths.
Source hashing excludes `target` (including `tools/xtask/target`), `target-*`,
`dist`, `.git`, Python caches and logs, and rejects symlinks or newline filenames.
Only the explicit root/source/vendor/license/tool/docs/workflow boundary is included.

Only macOS arm64 is packaged, with macOS 11+ declared; compatibility with older
macOS/terminals is not thereby validated. The executable is ad-hoc signed, not
Developer-ID signed or notarized. SHA256 is an integrity check, not an independent
publisher signature. The helper never changes Gatekeeper or quarantine settings.

## License collection

```sh
cargo metadata --locked --format-version=1 --filter-platform aarch64-apple-darwin > /tmp/dot-metadata.json
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- \
  collect-licenses /tmp/dot-metadata.json /tmp/dot-licenses.txt \
  --inventory /tmp/dot-license-inventory.json
```

Only normal and build dependencies reachable from the app root are collected;
dev-only dependencies are excluded. License expressions are metadata, not a claim
that every alternative applies. Texts are included verbatim with ordinary text
newline normalization. Missing texts fail closed; only exact-version overrides in
`licenses/crates` are accepted. Resolved paths must remain inside the package.
Embedded syntax/theme notices additionally require exact-version provenance and
matching SHA256 under `licenses/assets`; SPDX counts alone are insufficient.
Extracted-code notices and asset provenance are included in the output.

## Local executable replacement

```sh
cargo run --manifest-path tools/xtask/Cargo.toml --locked -- \
  publish-release /path/to/verified/dot-tui /absolute/destination/dot-tui
```

Despite its name, `publish-release` only replaces a **local file**, not a GitHub
release. It copies into a new inode in the destination directory, fsyncs, verifies
SHA256 and macOS code signature, then atomically renames. It refuses a symlink
destination and never truncates a running executable in place. It verifies the
result and inode change without launching the binary. Use a temporary destination
when validating the helper, not an installed user binary.

## Validation and CI

```sh
cargo test --manifest-path tools/xtask/Cargo.toml --locked
```

The manual-only `.github/workflows/release-build.yml` uses the pinned Rust
1.89.0 toolchain on the documented arm64 `macos-15` runner. It has read-only
repository permissions and retains a reviewable Actions artifact, not a public
release. Preparing a bundle does not authorize uploading it. Review the source
boundary, notices, version and asset checksums before publication.
