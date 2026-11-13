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
#    the layers listed before it. A missing layer directory is a renamed
#    layer — fail loudly rather than silently checking nothing.
DSP=crates/stave-dsp/src
layers=(units kernel primitives behavioral circuit physical learned effects structures)
# Non-test, non-comment references to crate::<layer> under a directory.
# Test modules may compose across layers (a resampler test can drive a
# triode as its fixture); the rule binds shipped code.
layer_refs() {
  find "$1" -name '*.rs' ! -name 'tests.rs' -print0 | xargs -0 awk -v pat="crate::$2([^a-z_]|$)" '
    FNR == 1 { in_test = 0 }
    /^#\[cfg\(test\)\]/ { in_test = 1 }
    !in_test && $0 !~ /^[[:space:]]*\/\// && $0 ~ pat { print FILENAME ":" FNR ": " $0 }'
}
for i in "${!layers[@]}"; do
  layer=${layers[$i]}
  if [[ ! -d "$DSP/$layer" ]]; then
    echo "LAYERING GATE BROKEN: $DSP/$layer is missing (update the layer list)"; fail=1; continue
  fi
  for above in "${layers[@]:$((i + 1))}"; do
    hits=$(layer_refs "$DSP/$layer" "$above")
    if [[ -n "$hits" ]]; then
      echo "LAYERING VIOLATION ($layer -> $above):"; echo "$hits"; fail=1
    fi
  done
done
exit $fail
