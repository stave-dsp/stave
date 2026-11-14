#!/usr/bin/env bash
# Binding and layering bars: engines publish through declared taps,
# and stave-dsp's layers point down.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
# 1) G6 grep bar: engines publish observations ONLY through declared
#    Tap handles — no name-string publishing at engine call sites.
hits=$(grep -rn "probe!\|probe_f64\|probe_named" examples/*/src --include="*.rs" || true)
if [[ -n "$hits" ]]; then
  echo "G6 VIOLATION: name-keyed tap publishing in an engine (declare a Tap):"; echo "$hits"; fail=1
fi
# 2) stave-dsp layering (DSP.md's layer table; the guide:
#    crates/stave-guide/src/concepts/dsp.md): each layer may use only
#    the layers listed before it. scripts/lib/layering.pl parses: a
#    `#[cfg(test)]` covers the one item after it (not the rest of the
#    file), and grouped `use crate::{…}` imports count. A missing layer
#    directory is a renamed layer — fail loudly rather than silently
#    checking nothing. Test and proof modules may compose across layers
#    (a resampler test can drive a triode as its fixture); the rule binds
#    shipped code.
layers=(units kernel primitives behavioral circuit physical learned effects structures)

# The parser must catch what it claims to: a fixture crate through the
# same script, so a broken pattern cannot pass everything silently.
# `want` is the exact set of shipped up-references in the fixture.
layering_selftest() {
  local dir got want
  dir=$(mktemp -d)
  mkdir -p "$dir/src/low/deep" "$dir/src/high"
  : > "$dir/src/high/mod.rs"
  cat > "$dir/src/low/mod.rs" <<'RS'
//! Mentions crate::high in a doc comment: not code.
use crate::low::deep::X;
#[cfg(test)]
mod tests {
    use crate::high::A;
    fn f() { let _ = '{'; }
}
use crate::high::Shipped1;
#[cfg(test)]
use crate::high::B;
pub fn g() -> &'static str { "crate::high::C {" }
use crate::{low::Y, high::Shipped2};
use crate::{
    low::{Z, W},
    high::{Shipped3, Shipped4},
};
#[cfg(any(test, kani))]
fn proof() { crate::high::D; }
#[cfg(test)]
mod probe;
#[cfg(any(test, feature = "x"))]
fn maybe() { crate::high::Shipped5; }
pub fn h() { crate::high::Shipped6(); }
RS
  echo 'use crate::high::TestOnlyFile;' > "$dir/src/low/probe.rs"
  printf '#![cfg(test)]\nuse crate::high::TestOnlyModule;\n' > "$dir/src/low/deep/mod.rs"
  got=$(perl scripts/lib/layering.pl "$dir/src" low high | sed -E 's|^.*/src/||; s|: low -> high:.*||' | tr '\n' ' ')
  want="low/mod.rs:8 low/mod.rs:12 low/mod.rs:15 low/mod.rs:22 low/mod.rs:23 "
  rm -rf "$dir"
  if [[ "$got" != "$want" ]]; then
    echo "LAYERING GATE BROKEN: scripts/lib/layering.pl flagged [${got}] in its fixture, not [${want}]"
    fail=1
  fi
}
layering_selftest

hits=$(perl scripts/lib/layering.pl crates/stave-dsp/src "${layers[@]}")
if grep -q ': missing layer ' <<<"$hits"; then
  echo "LAYERING GATE BROKEN (update the layer list):"; grep ': missing layer ' <<<"$hits"; fail=1
fi
if grep -v -e ': missing layer ' -e '^$' <<<"$hits" | grep -q .; then
  echo "LAYERING VIOLATION (shipped code using a layer above its own):"
  grep -v -e ': missing layer ' -e '^$' <<<"$hits"; fail=1
fi
exit $fail
