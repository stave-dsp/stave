#!/usr/bin/env bash
# Functional core, imperative shell (OVERHAUL.md V6): the pure
# crates — DSP (stave-dsp), layout and the Scene IR (stave-ui), the
# rasterizer (stave-raster), faces (stave-face), measurement
# (stave-measure), the protocol models (stave-model) — compute; they do
# not read clocks, files, the environment or randomness, start threads,
# keep global or hidden mutable state, or touch atomics and locks. So the
# same inputs give the same pixels and samples: golden images, replays
# and the simulated host hold. Effects live in the shell (stave-core's
# runtime, the format wrappers, stave-cocoa, stave-host, the tools).
#
# An exception is a line in ALLOWED below, with its reason.
#
#   scripts/check-purity.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PURE=(crates/stave-dsp/src crates/stave-ui/src crates/stave-raster/src
      crates/stave-face/src crates/stave-measure/src crates/stave-model/src
      crates/stave-font/src)
EFFECTS='std::time|Instant::now|SystemTime|std::thread|thread_local!|std::fs|std::env|std::process|std::net|getrandom|rand::|Mutex|RwLock|Atomic[A-Z][a-z0-9]+|static mut|OnceLock|LazyLock|OnceCell|RefCell|Cell<'

# path:pattern — why it may stay.
ALLOWED=(
  # Data-parallel bake: every pixel is a pure function of its position;
  # threads split rows, so the image is the same on any schedule.
  'crates/stave-face/src/material.rs:std::thread'
  # Design studies (DSP.md G2, G3): each evaluation a pure function of
  # its values, split over the caller's thread count and gathered by
  # index, so the numbers are the same on any schedule.
  'crates/stave-measure/src/design/mod.rs:std::thread'
  # The shared Levenberg–Marquardt (DSP.md E11, G4): the Jacobian's
  # columns, each a pure function of the point, split over the caller's
  # thread count and gathered by index (bit-identical on any count:
  # levenberg_marquardt::tests::threads_do_not_change_the_result).
  'crates/stave-measure/src/levenberg_marquardt.rs:std::thread'
  # Memoised bakes: a cache hit returns the image a miss would compute.
  'crates/stave-face/src/part.rs:RefCell'
  # Trace's scrolling history: one sample per frame time (6e07cd1), so a
  # frame painted twice paints the same, and a replayed frame sequence
  # paints the same pictures.
  'crates/stave-face/src/parts/meter.rs:Mutex'
)

hits=$(git grep -nIE "$EFFECTS" -- "${PURE[@]}" | grep -vE '^[^:]+:[0-9]+:\s*//' || true)
fail=0
while IFS= read -r hit; do
  [[ -z "$hit" ]] && continue
  file=${hit%%:*}
  ok=0
  for a in "${ALLOWED[@]}"; do
    if [[ "$file" == "${a%%:*}" && "$hit" =~ ${a#*:} ]]; then ok=1; fi
  done
  if [[ $ok == 0 ]]; then
    echo "effect in a pure crate: $hit"
    fail=1
  fi
done <<< "$hits"
if [[ $fail != 0 ]]; then
  echo "check-purity: move the effect to the shell, or add it to ALLOWED in $0 with its reason" >&2
  exit 1
fi
echo "check-purity: ok"
