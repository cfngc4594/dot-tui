#!/bin/sh
# Build an isolated offline demo, leaving the installed dot-tui unchanged.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
cargo build --locked --features dev-demo --target-dir "$ROOT/target-showcase"
exec "$ROOT/target-showcase/debug/dot-tui" --showcase "$@"
