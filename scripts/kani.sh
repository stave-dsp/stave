#!/usr/bin/env bash
# The Kani proof harnesses (OVERHAUL.md V2, DSP.md R10): bounded model
# checking of the unsafe and index-heavy core. Each `#[kani::proof]` (in
# `#[cfg(kani)] mod proofs`) states a property over EVERY input within
# its bounds; Kani also checks every pointer dereference, index and
# arithmetic operation on the way.
#
#   scripts/kani.sh                 # every harness
#   scripts/kani.sh -p stave-core   # one crate's
#   scripts/kani.sh -p stave-core --harness ranges_alias_is_interval_overlap
#
# Every harness runs under a time bound, STAVE_KANI_TIMEOUT (default 20m:
# the slowest harness takes a few minutes; Kani's suffixes s, m, h): one
# that blows its bounds fails with a timeout instead of running for hours.
#
# Tool: kani-verifier 0.68.0 (`cargo install --locked kani-verifier@0.68.0
# && cargo kani setup`), CI's `kani` job.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CRATES=(stave-core stave-telemetry stave-dsp)
BOUND=(-Z unstable-options --harness-timeout "${STAVE_KANI_TIMEOUT:-20m}")
if [[ $# -gt 0 ]]; then
  exec cargo kani "${BOUND[@]}" "$@"
fi
for crate in "${CRATES[@]}"; do
  echo "== kani: ${crate}"
  cargo kani "${BOUND[@]}" -p "$crate"
done
