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
#   scripts/kani.sh --list          # every harness, `crate harness` per line
#   scripts/kani.sh --matrix        # the same as CI's job matrix (deep.yml)
#
# The harnesses are found, not listed: every `#[kani::proof]` in a
# tracked crate source (scripts/lib/kani-harnesses.pl), so a new one runs
# here and in CI's nightly matrix without being registered anywhere.
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
harnesses() {
  local files=() f got dir
  # The parser must find what it claims to: a fixture first, so a broken
  # pattern cannot hand CI an empty or short matrix.
  dir="$(mktemp -d)"
  mkdir -p "$dir/crates/fixture/src"
  printf '%s\n' '#[cfg(kani)]' 'mod proofs {' '    #[kani::proof]' '    #[kani::unwind(4)]' \
    '    fn first() {}' '    #[kani::proof]' '    // a note' '    pub(crate) fn second() {}' '}' \
    '#[kani::proof]' 'fn third() {}' > "$dir/crates/fixture/src/lib.rs"
  got="$(perl scripts/lib/kani-harnesses.pl "$dir/crates/fixture/src/lib.rs" | tr '\n' ' ')"
  rm -rf "$dir"
  [[ "$got" == "fixture first fixture second fixture third " ]] ||
    { echo "kani.sh: scripts/lib/kani-harnesses.pl found [$got] in its fixture, not its 3 harnesses" >&2; return 1; }
  while IFS= read -r f; do files+=("$f"); done < <(git ls-files -z -- 'crates/*.rs' | xargs -0 grep -l 'kani::proof' || true)
  ((${#files[@]})) || { echo "kani.sh: no #[kani::proof] found" >&2; return 1; }
  perl scripts/lib/kani-harnesses.pl "${files[@]}"
}
case "${1:-}" in
  --list) harnesses; exit ;;
  --matrix)
    list="$(harnesses)"
    perl -ne 'chomp; my ($c, $h) = split; push @r, qq({"crate":"$c","harness":"$h"});
      END { print q({"include":[), join(",", @r), "]}\n" }' <<<"$list"
    exit ;;
esac
BOUND=(-Z unstable-options --harness-timeout "${STAVE_KANI_TIMEOUT:-20m}")
if [[ $# -gt 0 ]]; then
  exec cargo kani "${BOUND[@]}" "$@"
fi
list="$(harnesses)"
crates=()
while IFS= read -r c; do crates+=("$c"); done < <(cut -d' ' -f1 <<<"$list" | uniq)
for crate in "${crates[@]}"; do
  echo "== kani: ${crate}"
  cargo kani "${BOUND[@]}" -p "$crate"
done
