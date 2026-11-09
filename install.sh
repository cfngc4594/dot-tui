#!/bin/sh
# dot-tui release installer. Only macOS Apple Silicon is distributed here.
# Platform/Rosetta detection and stage-then-rename follow Grok Build 2bdd1d6
# scripts/install.sh (Apache-2.0); checksum/layout/login handling are Dot adapters.
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
    [ "$(/usr/bin/stat -f '%u' "$1")" = "$(/usr/bin/id -u)" ] || fail "Directory is not owned by you: $1"
    MODE=$(/usr/bin/stat -f '%Lp' "$1")
    case "$MODE" in ''|*[!0-7]*) fail 'Cannot identify directory permissions' ;; esac
    [ "$((0$MODE & 0022))" -eq 0 ] || fail "Directory is writable by other users: $1"
}

main() {
    set -eu
    umask 077
    DEFAULT_VERSION=0.8.4
    [ "$#" -le 1 ] || fail 'Usage: sh install.sh [0.8.4]'
    VERSION=${1:-$DEFAULT_VERSION}
    # Exactly three numeric components; never interpolate paths/URLs from shell code.
    printf '%s\n' "$VERSION" | /usr/bin/grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Expected a version such as 0.8.4'
    [ "$(uname -s)" = Darwin ] || fail 'This release supports macOS Apple Silicon only'
    ARCH=$(uname -m)
    case "$ARCH" in
        arm64|aarch64) ;;
        x86_64)
            # Original Grok Rosetta correction; no x86 binary is implied.
            [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] || fail 'Intel Mac packages are not available'
            ;;
        *) fail "Unsupported architecture: $ARCH" ;;
    esac
    OS_MAJOR=$(/usr/bin/sw_vers -productVersion | /usr/bin/cut -d. -f1)
    case "$OS_MAJOR" in ''|*[!0-9]*) fail 'Cannot identify macOS version' ;; esac
    [ "$OS_MAJOR" -ge 11 ] || fail 'macOS 11 or newer is required'
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
    ASSET=dot-tui-v${VERSION}-aarch64-apple-darwin.tar.gz
    BASE=https://github.com/cfngc4594/dot-tui/releases/download/v${VERSION}
    fetch "$BASE/SHA256SUMS" "$WORK/SHA256SUMS" || fail 'Cannot download release checksums; this version may not be published'
    EXPECTED=$(awk -v name="$ASSET" '$2 == name && NF == 2 { print $1 }' "$WORK/SHA256SUMS")
    [ "${#EXPECTED}" -eq 64 ] || fail 'Missing, duplicate or invalid archive checksum'
    case "$EXPECTED" in *[!0-9a-f]*) fail 'Invalid SHA256 checksum' ;; esac
    fetch "$BASE/$ASSET" "$WORK/$ASSET" || fail 'Cannot download release archive'
    ACTUAL=$(/usr/bin/shasum -a 256 "$WORK/$ASSET" | awk '{print $1}')
    [ "$ACTUAL" = "$EXPECTED" ] || fail 'Archive checksum mismatch; nothing installed'
    PACKED_VERSION=$(/usr/bin/tar -xOzf "$WORK/$ASSET" VERSION) || fail 'Missing archive version'
    [ "$PACKED_VERSION" = "$VERSION" ] || fail 'Archive version mismatch'
    # Extract only named streams: archive paths/symlinks never reach the filesystem.
    STAGED_BIN=$(mktemp "$BIN_DIR/.dot-tui.XXXXXXXX")
    /usr/bin/tar -xOzf "$WORK/$ASSET" dot-tui > "$STAGED_BIN" || fail 'Cannot extract executable'
    chmod 755 "$STAGED_BIN"
    /usr/bin/lipo "$STAGED_BIN" -verify_arch arm64 || fail 'Archive is not an arm64 executable'
    /usr/bin/codesign --verify --strict "$STAGED_BIN" || fail 'Executable signature validation failed'
    STAGED_LICENSES=$(mktemp -d "$DATA_DIR/.v${VERSION}.XXXXXXXX")
    for NAME in LICENSE THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.txt; do
        /usr/bin/tar -xOzf "$WORK/$ASSET" "$NAME" > "$STAGED_LICENSES/$NAME" || fail "Missing $NAME"
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
    case ":${PATH:-}:" in *":$BIN_DIR:"*) ;; *) printf 'Add %s to PATH in your shell configuration.\n' "$BIN_DIR" ;; esac
    printf 'Before running: sign in with official Codex using ChatGPT login and create your dot conversation in the official app.\n'
    printf 'dot-tui reads the existing Codex login; it does not ask for a token or support API-key login.\n'
    printf 'The installer has not started dot-tui or read your login.\n'
}

main "$@"
