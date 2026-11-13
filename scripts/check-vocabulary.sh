#!/usr/bin/env bash
# The glossary as a gate: one word per concept, one concept per word.
# The glossary (crates/stave-guide/src/glossary.md, the docs site's
# Glossary page) is the one source: this script reads its tables.
# Rejects
#
#   * every name in the glossary's "Retired names" table, anywhere in the
#     tree (code, scripts, docs) — the glossary itself and the overhaul
#     ledger, which records the history, excepted;
#   * the glossary's "never call it" words on the author surface (the
#     `stave` umbrella, stave-core's model, the guide, the examples),
#     where an author reads them first;
#   * a `Host` on the editor side (the face's side is the Controller;
#     Host is the DAW);
#   * an implementation word as the prefix or suffix of a name in
#     stave-dsp's public API (DSP.md naming rule 1): `Model`, `Impl`,
#     `Processor`, `Engine`, `Helper`, `V2`, `New`, `Simple`, `Fast`.
#
# Every word a rule below rejects must be listed in the "Never call it"
# column of the glossary's Terms table (checked first), so the gate
# cannot enforce a word the glossary does not document.
#
# "render" is checked across the whole tree: it means pixels, so only
# the code that makes pixels (the raster, the face, the platform view, the
# storybook, golden pictures, plots) says it.
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

GLOSSARY=crates/stave-guide/src/glossary.md
LEDGER=OVERHAUL.md

# table <heading>: the rows of the one table under `## <heading>`.
table() {
  awk -v h="## $1" '$0 == h {on = 1; next} /^## / {on = 0} on && /^\|/ && !/^\|[- |]*\|$/' "$GLOSSARY" | tail -n +2
}

# The Terms table's "Never call it" column, lower-cased, one cell a line.
NEVER=$(table Terms | awk -F'|' '{print tolower($4)}')
[[ -n "$NEVER" ]] || { echo "vocabulary: no Terms table in $GLOSSARY" >&2; exit 1; }

# documented <word...>: each word a rule rejects is in the glossary.
documented() {
  local w
  for w in "$@"; do
    if ! grep -qiE "(^|[ ,])${w}([ ,]|$)" <<<"$NEVER"; then
      printf 'vocabulary: the gate rejects "%s", but the glossary does not list it under "Never call it"\n' "$w" >&2
      fail=1
    fi
  done
}

# term <name>: the glossary defines the term the rule protects.
term() {
  if ! table Terms | awk -F'|' '{print $2}' | grep -qiw -- "$1"; then
    printf 'vocabulary: the gate protects "%s", but the glossary has no such term\n' "$1" >&2
    fail=1
  fi
}

# Retired names: gone from the API, and from every word about it. The
# first cell of each row of the glossary's "Retired names" table.
retired=$(table "Retired names" | awk -F'|' '{gsub(/[` ]/, "", $2); print $2}' | grep -v '^$')
[[ -n "$retired" ]] || { echo "vocabulary: no Retired names table in $GLOSSARY" >&2; exit 1; }
RETIRED="\\b($(paste -sd'|' - <<<"$retired"))\\b"
report "a retired name (the glossary names its replacement)" \
  "$RETIRED" -- ":!$LEDGER" ":!$GLOSSARY" ':!scripts/check-vocabulary.sh' ':!*.lock'

# The editor contract is the Controller; Host means the DAW (the format
# wrappers' handle on it, stave-host's hosting), never a face's side.
term Host
term Controller
report "\`Host\` names the DAW; the editor's side is \`Controller\`" \
  '\b(trait|struct|enum|dyn|impl) +Host\b|\bface::Host\b' -- crates/stave/ crates/stave-core/ crates/stave-face/ crates/stave-ui/ crates/stave-ui-cocoa/ crates/stave-ui-win32/ crates/stave-ui-x11/ crates/stave-ui-shell/ crates/stave-raster/ examples/ tools/storybook/

# The author surface: what a plugin author reads.
AUTHOR=(crates/stave/ crates/stave-core/src/ crates/stave-guide/ examples/ ':!crates/stave-core/src/runtime/' ":!$GLOSSARY")

PIXELS=(':!crates/stave-raster/' ':!crates/stave-face/' ':!crates/stave-ui/' ':!crates/stave-ui-cocoa/'
  ':!crates/stave-ui-win32/' ':!crates/stave-ui-x11/' ':!crates/stave-ui-shell/' ':!crates/stave-font/'
  ':!tools/storybook/' ':!crates/stave-test/src/face/' ':!crates/stave-test/src/image.rs' ':!*/tests/golden.rs'
  ':!scripts/plot_silk_atlas.py' ':!fuzz/fuzz_targets/raster.rs' ":!$LEDGER" ":!$GLOSSARY" ':!scripts/check-vocabulary.sh' ':!*.lock')
term render
report "\"render\" means pixels; audio is processed (process, run)" \
  -i '(?<!")\brender(s|ed|ing)?\b(?!")' -- . "${PIXELS[@]}"
# (diagnostics report what a host did, in the host ABI's words.)
documented activate deactivate activation
report "the lifecycle is prepare / reset / process (activate is the wrappers')" \
  -i '\b(activate|deactivate|activation|activated)\b' -- "${AUTHOR[@]}" ':!crates/stave-core/src/diagnostics/'
documented engine processor
report "the author's type is a Plugin (not an engine, processor or spec)" \
  -i '\b(engines?|processors?|StaveEngine)\b' -- "${AUTHOR[@]}"
documented sr fs sample_offset
report "sample_rate, never sr or fs; frame, never sample_offset" \
  '(\b(sr|fs)\b[^:]|\bsample_offset\b)' -- "${AUTHOR[@]}" ':!crates/stave-core/src/diagnostics/'
documented plain get index
report "a param's value is value(), in its natural type" \
  '\.(plain|get|index)\(\)' -- crates/stave/ examples/

# stave-dsp names the thing, not the implementation (DSP.md naming
# rule 1): no implementation word as the prefix or suffix of a public
# type, trait or alias, or of a name re-exported `as`. (`NewtonSettings`
# passes: the word must end at a capital, a digit or the name's end.)
term model
documented model impl processor engine helper v2 new simple fast
IMPL='(Model|Impl|Processor|Engine|Helper|V2|New|Simple|Fast)'
NAME="(${IMPL}(?:[A-Z0-9]\\w*)?|\\w*[a-z0-9]${IMPL})\\b"
report "stave-dsp names the thing, not the implementation (DSP.md naming rule 1)" \
  "\\bpub +(struct|enum|trait|type|union) +${NAME}|\\bpub +use +[^;]*\\bas +${NAME}|^ *[\\w:]+ +as +${NAME} *[,}]" -- crates/stave-dsp/src/

exit "$fail"
