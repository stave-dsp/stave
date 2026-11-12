#!/usr/bin/env bash
# The fuzz targets (docs/overhaul.md C5), cargo-fuzz on the pinned
# nightly. Every target for SECONDS each (default 30: the PR smoke; the
# nightly job runs 600). The committed state fixtures seed state_load.
#
#   scripts/fuzz.sh            # every target, 30 s each
#   scripts/fuzz.sh 600        # every target, 10 min each
#   scripts/fuzz.sh 60 raster  # one target
#
# Tools: rustup toolchain nightly-2026-11-01, cargo-fuzz 0.13.2. A crash
# leaves its input in fuzz/artifacts/<target>/; replay it with
#   cargo +nightly-2026-11-01 fuzz run <target> fuzz/artifacts/<target>/<file>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
SECONDS_EACH="${1:-30}"
shift || true
NIGHTLY="${NIGHTLY:-nightly-2026-11-01}"
TARGET="$(rustc +"$NIGHTLY" -vV | sed -n 's/^host: //p')"
TARGETS=("$@")
[[ $# -gt 0 ]] || TARGETS=(state_load param_text vst3_host raster)
mkdir -p fuzz/corpus/state_load
cp crates/stave-core/tests/fixtures/*.stve fuzz/corpus/state_load/ 2>/dev/null || true
for target in ${TARGETS[@]+"${TARGETS[@]}"}; do
  echo "== fuzz: ${target} (${SECONDS_EACH}s)"
  # --target: a cargo-fuzz built for another arch (Rosetta) would
  # otherwise default to its own.
  cargo +"$NIGHTLY" fuzz run --fuzz-dir fuzz --target "$TARGET" "$target" -- \
    -max_total_time="$SECONDS_EACH" -rss_limit_mb=4096 -max_len=4096
done
