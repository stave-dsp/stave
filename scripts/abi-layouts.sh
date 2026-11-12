#!/usr/bin/env bash
# Print sizeof/alignof of every VST3 and CLAP struct stave mirrors, measured
# from the official headers — the numbers pinned by the `const` layout
# assertions in crates/stave-vst3/src/sys.rs and crates/stave-clap/src/sys.rs.
# Headers: target/vst3sdk (scripts/validate-formats.sh builds it) and
# target/clap (git clone https://github.com/free-audio/clap target/clap).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
c++ -std=c++17 -I "$ROOT/target/vst3sdk" "$ROOT/scripts/abi/vst3_layouts.cpp" -o "$OUT/vst3"
cc -std=c11 -I "$ROOT/target/clap/include" "$ROOT/scripts/abi/clap_layouts.c" -o "$OUT/clap"
echo "== VST3 (name size align)"; "$OUT/vst3"
echo "== CLAP (name size align)"; "$OUT/clap"
