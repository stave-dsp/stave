#!/usr/bin/env bash
# Refresh crates/stave-dsp/datasheet.json (DSP.md P1): run the primitive
# datasheet (crates/stave-dsp/tests/datasheet.rs) in release on the
# reference machine and write its rows, with the machine, CPU, commit and
# date they were measured at. The docs site's Cost page is generated from
# that file; the datasheet test fails, debug or release, whenever the
# stages it benches and the file's stages differ, so run this whenever a
# bench line is added, renamed or removed (and when a stage's cost moves).
#
#   scripts/datasheet.sh               # on the reference machine
#   scripts/datasheet.sh HOST          # elsewhere, through scripts/offload.sh
#   scripts/datasheet.sh local         # on this machine
#
# The reference machine is the one named by STAVE_REFERENCE_HOST in your
# local configuration (stave.env.example), described on the Cost page by
# STAVE_REFERENCE_NAME (the hardware, e.g. "Apple M1, 8 cores"). Every
# committed row comes from one machine, so rows compare with each other:
# measure elsewhere to explore, but commit numbers from it, and measure when
# it is quiet — the file records the load average at the start, and the Cost
# page shows it beside each row's spread. STAVE_DATASHEET_MACHINE overrides
# the name written for a one-off run.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source scripts/lib/config.sh
stave_config_load "$ROOT"
if [[ -n "${1:-}" ]]; then
  HOST="$1"
else
  HOST="$(stave_config_require STAVE_REFERENCE_HOST "datasheet.sh (the reference machine)")"
fi
REFERENCE_HOST="${STAVE_REFERENCE_HOST:-}"
REFERENCE_NAME="${STAVE_REFERENCE_NAME:-reference machine}"

if [[ -n "$REFERENCE_HOST" && "$HOST" == "$REFERENCE_HOST" ]]; then
  MACHINE="${STAVE_DATASHEET_MACHINE:-$REFERENCE_NAME}"
elif [[ "$HOST" == local ]]; then
  MACHINE="${STAVE_DATASHEET_MACHINE:-$(hostname -s)}"
else
  MACHINE="${STAVE_DATASHEET_MACHINE:-$HOST}"
fi
COMMIT="$(git rev-parse --short HEAD)"
git diff --quiet HEAD -- crates/stave-dsp ':!crates/stave-dsp/datasheet.json' || COMMIT="$COMMIT+dirty"
RUN=(env STAVE_DATASHEET_WRITE=1 STAVE_DATASHEET_MACHINE="$MACHINE"
  STAVE_DATASHEET_COMMIT="$COMMIT" STAVE_DATASHEET_DATE="$(date -u +%F)"
  cargo test --release --locked -p stave-dsp --test datasheet -- --nocapture --test-threads 1)

if [[ "$HOST" == local ]]; then
  "${RUN[@]}"
else
  STAVE_OFFLOAD_FETCH=crates/stave-dsp/datasheet.json \
    STAVE_OFFLOAD_NAME="${STAVE_OFFLOAD_NAME:-datasheet}" \
    scripts/offload.sh "$HOST" -- "${RUN[@]}"
fi
echo "datasheet.sh: crates/stave-dsp/datasheet.json measured on $MACHINE at $COMMIT"
