#!/usr/bin/env bash
# The glossary (docs/overhaul.md, "Glossary (normative)") as a gate: one
# word per concept, one concept per word. Rejects
#
#   * retired names, anywhere in the tree (code, scripts, docs) — the
#     overhaul ledger, which records the history, is the one exception;
#   * the glossary's "never call it" words on the author surface (the
#     `stave` umbrella, stave-core's model, the examples), where an author
#     reads them first;
#   * a `Host` on the editor side (the face's side is the Controller;
#     Host is the DAW).
#
# Words still used for another concept elsewhere ("render" for audio in
# stave-host, stave-measure and the test drivers) are the T6 ledger item;
# when it lands, widen AUTHOR to the whole tree.
#
#   scripts/check-vocabulary.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

# report <rule> <git grep args...>: every match is a failure.
report() {
  local rule=$1
  shift
  local hits
  hits=$(git grep -nIP "$@" || true)
  if [[ -n "$hits" ]]; then
    printf 'vocabulary: %s\n%s\n\n' "$rule" "$hits" >&2
    fail=1
  fi
}

# Retired names: gone from the API, and from every word about it.
RETIRED='\b(DebugBus|debug_bus|stave_debug|FaceSpec|HasFace|StavePlugin|ProcessCtx|PluginDescriptor|PluginFeature|PortConfig|AuxIn|AudioBuffer|ContinuousControl|ContinuousBlock|DiscreteControl|ReconfigureControl|continuous_controls|attach_telemetry|debug_attach|param_table|save_extra|load_extra|stave_vst3_export|stave_clap_export|EditorHost|editor_face|editor_ui|UiNode|latency_samples|tail_samples)\b'
report "a retired name (see the glossary for its replacement)" \
  "$RETIRED" -- ':!docs/overhaul.md' ':!scripts/check-vocabulary.sh' ':!*.lock'

# The editor contract is the Controller; Host means the DAW (the format
# wrappers' handle on it, stave-host's hosting), never a face's side.
report "\`Host\` names the DAW; the editor's side is \`Controller\`" \
  '\b(trait|struct|enum|dyn|impl) +Host\b|\bface::Host\b' -- crates/stave/ crates/stave-core/ crates/stave-face/ crates/stave-ui/ crates/stave-ui-cocoa/ crates/stave-raster/ examples/ tools/storybook/

# The author surface: what a plugin author reads.
AUTHOR=(crates/stave/ crates/stave-core/src/ examples/ ':!crates/stave-core/src/runtime/')

report "\"render\" means pixels; audio is processed (process, run)" \
  -i '\brender(s|ed|ing)?\b' -- "${AUTHOR[@]}"
report "the lifecycle is prepare / reset / process (activate is the wrappers')" \
  -i '\b(activate|deactivate|activation|activated)\b' -- "${AUTHOR[@]}"
report "the author's type is a Plugin (not an engine, processor or spec)" \
  -i '\b(engines?|processors?|StaveEngine)\b' -- "${AUTHOR[@]}"
report "sample_rate, never sr or fs; frame, never sample_offset" \
  '(\b(sr|fs)\b[^:]|\bsample_offset\b)' -- "${AUTHOR[@]}" ':!crates/stave-core/src/diagnostics/'
report "a param's value is value(), in its natural type" \
  '\.(plain|get|index)\(\)' -- crates/stave/ examples/

exit "$fail"
