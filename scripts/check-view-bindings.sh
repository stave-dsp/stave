#!/usr/bin/env bash
# The F11 grep bar: no CONTROL-name binding in view/framework code.
# Allowed name-keyed surfaces (their names ARE the API):
#   - probes (find_probe / probes[i].name_str)
#   - DebugBus preset pairs (project-declared, resolved from typed ids)
#   - binding.rs's index-identity debug_assert
# Everything else binds by ParamId through the owned tree.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
# 1) find_control: only its definition (mapping.rs) and the preset
#    apply path (tick.rs) may reference it.
hits=$(grep -rn "find_control" crates/stave-ui-cocoa/src crates/stave-vst3/src --include="*.rs" \
  | grep -v "mapping.rs" | grep -v "tick.rs" || true)
if [[ -n "$hits" ]]; then
  echo "F11 VIOLATION: control-name lookup outside allowed files:"; echo "$hits"; fail=1
fi
# 2) controls[..].name_str() comparisons anywhere in framework view code
hits=$(grep -rn "controls\[[a-z]*\]\.name_str() ==" crates/stave-ui-cocoa/src crates/stave-vst3/src --include="*.rs" \
  | grep -v "binding.rs" | grep -v "mapping.rs" || true)
if [[ -n "$hits" ]]; then
  echo "F11 VIOLATION: control-name comparison in framework code:"; echo "$hits"; fail=1
fi
# 3) G6 grep bar: engines publish observations ONLY through declared
#    Tap handles — no name-string publishing at engine call sites.
hits=$(grep -rn "probe!\|probe_f64\|probe_named" plugins/*/src --include="*.rs" || true)
if [[ -n "$hits" ]]; then
  echo "G6 VIOLATION: name-keyed tap publishing in an engine (declare a Tap):"; echo "$hits"; fail=1
fi
# 4) stave-dsp layering (docs/architecture.md): each layer may use only
#    the layers listed before it. A missing layer directory is a renamed
#    layer — fail loudly rather than silently checking nothing.
DSP=crates/stave-dsp/src
layers=(kernel primitives behavioral circuit structures)
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
