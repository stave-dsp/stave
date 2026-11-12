#!/usr/bin/env bash
# The hygiene gates (OVERHAUL.md C9) beside the vocabulary check:
#
#   scripts/hygiene.sh             # licences, duplicate versions, sources,
#                                  # unused dependencies, rustdoc links
#   scripts/hygiene.sh deps        # the first two only (no compiling)
#   scripts/hygiene.sh advisories  # the RustSec database (CI: nightly —
#                                  # it changes without our commits)
#
# Tools (CI installs these exact versions): cargo-deny 0.20.2,
# cargo-machete 0.9.2. Policy: deny.toml.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "${1:-}" == advisories ]]; then
  exec cargo deny --locked check advisories
fi

cargo deny --locked check bans licenses sources
cargo machete $(ls -d crates/*/ tools/*/ examples/*/ fuzz/)
# `deps`: no compiling (CI's Linux `rules` job; the macOS `lint` job
# documents the private items).
[[ "${1:-}" == deps ]] && exit 0
# Every intra-doc link resolves, private items' docs included.
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked
