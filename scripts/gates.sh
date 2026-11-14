#!/usr/bin/env bash
# The commit gate: tree rules, then the tests — every test in parallel
# (cargo-nextest), plus doctests (nextest does not run those).
#
#   scripts/gates.sh            # everything
#   scripts/gates.sh affected   # only packages the uncommitted change can reach
#   scripts/gates.sh affected main   # ... the branch's change since `main`
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
bash scripts/check-file-sizes.sh
bash scripts/check-view-bindings.sh
bash scripts/check-plugin-rules.sh
bash scripts/check-vocabulary.sh
bash scripts/check-purity.sh
bash scripts/check-golden-sizes.sh
bash scripts/test-offload-events.sh
if [[ "${1:-}" == affected ]]; then
  python3 scripts/test-affected.py "${2:-HEAD}"
else
  cargo nextest run --workspace
fi
cargo test --workspace --doc --quiet
