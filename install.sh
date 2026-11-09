#!/bin/sh
# dot-tui release installer for macOS, Linux and Git for Windows.
# Platform/Rosetta detection references https://x.ai/cli/install.sh (2026-11-09).
# Checksums, private paths and stage-then-rename are Dot installation safeguards.
# All work lives in main: a truncated piped download cannot execute a partial body.

fail() { printf 'dot-tui: %s\n' "$*" >&2; exit 1; }

cleanup() {
    [ -z "${STAGED_BIN:-}" ] || rm -f "$STAGED_BIN"
    [ -z "${STAGED_LICENSES:-}" ] || rm -rf "$STAGED_LICENSES"
    [ -z "${WORK:-}" ] || rm -rf "$WORK"
}

fetch() {
    curl --disable --fail --silent --show-error --location \
        --proto '=https' --proto-redir '=https' --tlsv1.2 \
        --connect-timeout 15 --max-time 180 --retry 2 \
        --output "$2" "$1"
}

require_user_directory() {
    [ ! -L "$1" ] || fail "Install directory must not be a symlink: $1"
    [ "$(if [ "$OS" = Darwin ]; then stat -f '%u' "$1"; else stat -c '%u' "$1"; fi)" = "$(id -u)" ] || fail "Directory is not owned by you: $1"
    MODE=$(if [ "$OS" = Darwin ]; then stat -f '%Lp' "$1"; else stat -c '%a' "$1"; fi)
    case "$MODE" in ''|*[!0-7]*) fail 'Cannot identify directory permissions' ;; esac
    [ "$((0$MODE & 0022))" -eq 0 ] || fail "Directory is writable by other users: $1"
}

main() {
    set -eu
    umask 077
    DEFAULT_VERSION=0.8.6
    [ "$#" -le 1 ] || fail 'Usage: sh install.sh [X.Y.Z]'
    VERSION=${1:-$DEFAULT_VERSION}
    # Exactly three numeric components; never interpolate paths/URLs from shell code.
    printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Expected a version such as 0.8.5'
    OS=$(uname -s)
    ARCH=$(uname -m)
    case "$ARCH" in
        arm64|aarch64|ARM64) ARCH=aarch64 ;;
        x86_64|amd64|AMD64) ARCH=x86_64 ;;
        *) fail "Unsupported architecture: $ARCH" ;;
    esac
    case "$OS" in
        Darwin)
            if [ "$ARCH" = x86_64 ] && [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then ARCH=aarch64; fi
            TARGET=$ARCH-apple-darwin
            OS_MAJOR=$(sw_vers -productVersion | cut -d. -f1)
            case "$OS_MAJOR" in ''|*[!0-9]*) fail 'Cannot identify macOS version' ;; esac
            [ "$OS_MAJOR" -ge 11 ] || fail 'macOS 11 or newer is required'
            ;;
        Linux)
            TARGET=$ARCH-unknown-linux-gnu
            LIBC=$(getconf GNU_LIBC_VERSION 2>/dev/null) || fail 'Linux requires glibc 2.39 or newer; musl/unknown libc is unsupported'
            printf '%s\n' "$LIBC" | grep -Eq '^glibc [0-9]+\.[0-9]+$' || fail 'Cannot identify glibc version; Linux requires glibc 2.39 or newer'
            LIBC_VERSION=${LIBC#glibc }
            LIBC_MAJOR=${LIBC_VERSION%%.*}
            LIBC_MINOR=${LIBC_VERSION#*.}
            if ! [ "$LIBC_MAJOR" -gt 2 ] && ! { [ "$LIBC_MAJOR" -eq 2 ] && [ "$LIBC_MINOR" -ge 39 ]; }; then
                fail "Linux requires glibc 2.39 or newer; found $LIBC"
            fi
            ;;
        MINGW*|MSYS*|CYGWIN*)
            [ -z "${DOT_TUI_INSTALL_DIR:-}" ] || fail 'On Windows use install.ps1 -InstallDir with an absolute Windows path'
            command -v curl >/dev/null 2>&1 || fail 'curl is required'
            command -v powershell.exe >/dev/null 2>&1 || fail "PowerShell is required"
            command -v cygpath >/dev/null 2>&1 || fail "cygpath is required"
            WORK=$(mktemp -d "${TMPDIR:-/tmp}/dot-tui-install.XXXXXXXX")
            trap cleanup EXIT
            trap 'exit 1' HUP INT TERM
            fetch "https://raw.githubusercontent.com/cfngc4594/dot-tui/v${VERSION}/install.ps1" "$WORK/install.ps1" || fail "Cannot download versioned Windows installer"
            powershell.exe -NoProfile -File "$(cygpath -w "$WORK/install.ps1")" -Version "$VERSION"
            return

            ;;
        *) fail "Unsupported operating system: $OS" ;;
    esac
    command -v curl >/dev/null 2>&1 || fail 'curl is required'
    [ "$(id -u)" -ne 0 ] || fail 'Run as your own user, without sudo'
    : "${HOME:?HOME must be set}"
    BIN_DIR=${DOT_TUI_INSTALL_DIR:-$HOME/.local/bin}
    DATA_DIR=$HOME/.local/share/dot-tui/releases
    case "$BIN_DIR" in /*) ;; *) fail 'DOT_TUI_INSTALL_DIR must be absolute' ;; esac
    DEST=$BIN_DIR/dot-tui
    [ ! -L "$DEST" ] || fail "Refusing to replace symlink: $DEST"
    [ ! -e "$DEST" ] || [ -f "$DEST" ] || fail "Not a regular file: $DEST"
    [ ! -L "$DATA_DIR" ] || fail 'Release metadata directory must not be a symlink'
    [ ! -L "$HOME/.local/share/dot-tui" ] || fail 'dot-tui data directory must not be a symlink'
    mkdir -p "$BIN_DIR" "$DATA_DIR"
    require_user_directory "$BIN_DIR"
    require_user_directory "$DATA_DIR"
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/dot-tui-install.XXXXXXXX")
    STAGED_BIN=
    STAGED_LICENSES=
    trap cleanup EXIT
    trap 'exit 1' HUP INT TERM
    ASSET=dot-tui-v${VERSION}-${TARGET}.tar.gz
    BASE=https://github.com/cfngc4594/dot-tui/releases/download/v${VERSION}
    fetch "$BASE/SHA256SUMS" "$WORK/SHA256SUMS" || fail 'Cannot download release checksums; this version may not be published'
    EXPECTED=$(awk -v name="$ASSET" '$2 == name && NF == 2 { print $1 }' "$WORK/SHA256SUMS")
    [ "${#EXPECTED}" -eq 64 ] || fail 'Missing, duplicate or invalid archive checksum'
    case "$EXPECTED" in *[!0-9a-f]*) fail 'Invalid SHA256 checksum' ;; esac
    fetch "$BASE/$ASSET" "$WORK/$ASSET" || fail 'Cannot download release archive'
    if command -v sha256sum >/dev/null 2>&1; then ACTUAL=$(sha256sum "$WORK/$ASSET" | awk '{print $1}'); else ACTUAL=$(shasum -a 256 "$WORK/$ASSET" | awk '{print $1}'); fi
    [ "$ACTUAL" = "$EXPECTED" ] || fail 'Archive checksum mismatch; nothing installed'
    PACKED_VERSION=$(tar -xOzf "$WORK/$ASSET" VERSION) || fail 'Missing archive version'
    [ "$PACKED_VERSION" = "$VERSION" ] || fail 'Archive version mismatch'
    # Extract only named streams: archive paths/symlinks never reach the filesystem.
    STAGED_BIN=$(mktemp "$BIN_DIR/.dot-tui.XXXXXXXX")
    tar -xOzf "$WORK/$ASSET" dot-tui > "$STAGED_BIN" || fail 'Cannot extract executable'
    [ -s "$STAGED_BIN" ] || fail 'Empty executable'
    if [ "$OS" = Linux ]; then
        MAGIC=$(od -An -tx1 -N6 "$STAGED_BIN" | tr -d ' \n')
        [ "$MAGIC" = 7f454c460201 ] || fail 'Expected little-endian ELF64 executable'
        MACHINE=$(od -An -tx1 -j18 -N2 "$STAGED_BIN" | tr -d ' \n')
        case "$ARCH:$MACHINE" in x86_64:3e00|aarch64:b700) ;; *) fail 'Executable architecture mismatch' ;; esac
    fi
    chmod 755 "$STAGED_BIN"
    if [ "$OS" = Darwin ]; then
        MACH_ARCH=$ARCH
        [ "$ARCH" != aarch64 ] || MACH_ARCH=arm64
        /usr/bin/lipo "$STAGED_BIN" -verify_arch "$MACH_ARCH" || fail 'Executable architecture mismatch'
        /usr/bin/codesign --verify --strict "$STAGED_BIN" || fail 'Executable signature validation failed'
    fi
    # Clap exits on --version before runtime/auth initialization.
    BINARY_VERSION=$("$STAGED_BIN" --version) || fail 'Executable version check failed; previous installation retained'
    [ "$BINARY_VERSION" = "dot-tui $VERSION" ] || fail 'Executable version mismatch; previous installation retained'
    STAGED_LICENSES=$(mktemp -d "$DATA_DIR/.v${VERSION}.XXXXXXXX")
    for NAME in LICENSE THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.txt; do
        tar -xOzf "$WORK/$ASSET" "$NAME" > "$STAGED_LICENSES/$NAME" || fail "Missing $NAME"
        [ -s "$STAGED_LICENSES/$NAME" ] || fail "Empty $NAME"
    done
    printf '%s\n' "$EXPECTED" > "$STAGED_LICENSES/PACKAGE_SHA256"
    RELEASE_DIR=$DATA_DIR/v$VERSION
    [ ! -L "$RELEASE_DIR" ] || fail 'Existing release metadata is a symlink'
    if [ -e "$RELEASE_DIR" ]; then
        [ -d "$RELEASE_DIR" ] && [ -f "$RELEASE_DIR/PACKAGE_SHA256" ] || fail 'Existing release metadata is invalid'
        [ "$(cat "$RELEASE_DIR/PACKAGE_SHA256")" = "$EXPECTED" ] || fail 'Version already installed with a different archive; use a new version'
    else
        mv "$STAGED_LICENSES" "$RELEASE_DIR"
        STAGED_LICENSES=
    fi
    # New inode in the destination filesystem: never truncate running/signed code.
    # Recheck the final path before replacing it; no Grok/agent path is touched.
    [ ! -L "$DEST" ] || fail 'Destination became a symlink'
    [ ! -e "$DEST" ] || [ -f "$DEST" ] || fail 'Destination is no longer a regular file'
    mv -f "$STAGED_BIN" "$DEST"
    STAGED_BIN=
    printf 'Installed dot-tui %s: %s\n' "$VERSION" "$DEST"
    printf 'Licenses: %s\n' "$RELEASE_DIR"
    case ":${PATH:-}:" in
        *":$BIN_DIR:"*) ;;
        *)
            printf 'Add this directory to PATH in your shell configuration: %s\n' "$BIN_DIR"
            ;;
    esac
    printf 'Before running: sign in with official Codex using ChatGPT login and create your dot conversation in the official app.\n'
    printf 'dot-tui reads the existing Codex login; it does not ask for a token or support API-key login.\n'
    printf 'The installer ran only dot-tui --version; it did not open the TUI or read your login.\n'
}

main "$@"
