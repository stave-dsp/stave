#!/usr/bin/env bash
# Build + bundle with FRESHNESS PROOF — the only sanctioned path to
# a bench-measurable bundle. Born on the poisoned-measurement day
# (2026-07-31): stale bundles + silent no-op edits produced hours
# of fictional measurements. This script makes each link loud:
#   1. cargo build --release (errors fatal, output shown)
#   2. dylib must be at least as new as every source in the crate
#   3. bundle, then print the bundle binary's hash and the plugin's
#      self-reported `build` probe (compile timestamp)
# Measurement scripts MUST print/compare the build probe per run.
set -euo pipefail

CRATE="${1:?usage: fresh-bundle.sh <crate-name> [crate-dir]}"
CRATE_DIR="${2:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo build --release -p "$CRATE"

DYLIB="target/release/lib${CRATE//-/_}.dylib"
if [[ -n "$CRATE_DIR" ]]; then
  NEWEST_SRC=$(find "$CRATE_DIR" -name '*.rs' -newer "$DYLIB" | head -1)
  if [[ -n "$NEWEST_SRC" ]]; then
    echo "STALE: $NEWEST_SRC is newer than $DYLIB" >&2
    exit 1
  fi
fi

VST3=$(bash scripts/bundle-vst3.sh "$CRATE" release | sed -n 's/^built: //p' | head -1)
BASE=$(basename "$VST3" .vst3)
HASH=$(md5 -q "$VST3/Contents/MacOS/$BASE")
PROBE=$(./target/release/stave-bench "$VST3" --rate 48000 --seconds 0.2 --signal silence 2>/dev/null | grep -E "^\s+build\s" | awk '{print $2}')
echo "FRESH: $VST3"
echo "  binary md5   $HASH"
echo "  build probe  ${PROBE:-<none — plugin lacks the stamp>}"
