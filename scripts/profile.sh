#!/usr/bin/env bash
# A flamegraph of any stage, test or plugin bench (DSP.md P3): samply on
# macOS, perf on Linux. Builds in the `profiling` profile (release with
# line tables and symbols kept), records, and opens the result.
#
#   scripts/profile.sh stage triode                 # a stage of crates/stave-dsp/benches/stages.rs
#   scripts/profile.sh --seconds 30 stage amp-chain
#   scripts/profile.sh test stave-dsp datasheet primitive_datasheet   # PKG TEST [FILTER]
#   scripts/profile.sh bench stage:coupler --seconds 10    # stave-bench, any of its arguments
#   scripts/profile.sh -- target/release/my-tool --flag    # any command
#
#   options (before the target):
#     --out DIR       where the profile goes (target/profile)
#     --seconds N     how long a stage runs (10)
#     --no-open       record only (the default when stdout is not a terminal)
#
# macOS: samply (cargo install --locked samply) writes DIR/NAME.json.gz,
# opened in the Firefox Profiler (its Flame Graph tab) by `samply load`.
# Linux: perf writes DIR/NAME.perf.data and, with inferno installed
# (cargo install --locked inferno), DIR/NAME.svg. perf needs
# kernel.perf_event_paranoid <= 2 and the perf_event_open syscall, which
# unprivileged containers (a common build-host setup) do not grant:
# profile on a Mac, a Linux machine you own, or a GitHub runner.
#
# On a build host, fetching the profile back:
#   STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh macos -- scripts/profile.sh stage triode
#   samply load target/profile/stage-triode.json.gz
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT=target/profile
SECONDS_RUN=10
OPEN=1
[[ -t 1 ]] || OPEN=0
usage() { sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
while (($#)); do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --seconds) SECONDS_RUN="$2"; shift 2 ;;
    --no-open) OPEN=0; shift ;;
    -h | --help) usage ;;
    *) break ;;
  esac
done
(($#)) || usage

# The executable cargo builds for a target, in the profiling profile
# (from cargo's "Executable … (PATH)" line).
built() {
  cargo "$@" --locked --color never --profile profiling 2>&1 \
    | tee /dev/stderr | sed -n 's/^ *Executable .*(\(.*\))$/\1/p' | tail -1
}

KIND="$1"
shift
case "$KIND" in
  stage)
    (($#)) || usage
    BIN="$(built bench -p stave-dsp --bench stages --no-run)"
    NAME="stage-$1"
    CMD=("$BIN" run "$1" "$SECONDS_RUN")
    ;;
  test)
    (($# >= 2)) || usage
    BIN="$(built test -p "$1" --test "$2" --no-run)"
    NAME="test-$2${3:+-$3}"
    CMD=("$BIN" --nocapture --test-threads 1 ${3:+"$3"})
    ;;
  bench)
    cargo build --locked --profile profiling -p stave-bench
    NAME="bench-$(basename "${1:-plugin}" | tr -c 'A-Za-z0-9._\n-' _)"
    CMD=(target/profiling/stave-bench "$@")
    ;;
  --)
    (($#)) || usage
    NAME="$(basename "$1")"
    CMD=("$@")
    ;;
  *) usage ;;
esac
[[ -x "${CMD[0]}" ]] || { echo "profile: nothing to run (${CMD[0]})" >&2; exit 1; }
mkdir -p "$OUT"

if [[ "$(uname)" == Darwin ]]; then
  command -v samply >/dev/null || { echo "profile: needs samply (cargo install --locked samply)" >&2; exit 2; }
  PROFILE="$OUT/$NAME.json.gz"
  # Symbols ride along in a .syms.json sidecar, so a profile fetched from
  # a build host opens with function names without its binaries.
  samply record --save-only --unstable-presymbolicate --output "$PROFILE" -- "${CMD[@]}"
  echo "profile: $PROFILE (open with: samply load $PROFILE)"
  ((OPEN)) && exec samply load "$PROFILE"
  exit 0
fi

command -v perf >/dev/null || { echo "profile: needs perf (linux-tools for this kernel)" >&2; exit 2; }
paranoid="$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 4)"
if ((paranoid > 2)) && ((EUID != 0)); then
  echo "profile: kernel.perf_event_paranoid is $paranoid; perf needs <= 2 (sudo sysctl kernel.perf_event_paranoid=2)." >&2
  echo "         An unprivileged container cannot change it: profile on a Mac or a Linux machine you own." >&2
  exit 2
fi
DATA="$OUT/$NAME.perf.data"
perf record -F 997 -g --call-graph dwarf -o "$DATA" -- "${CMD[@]}"
if command -v inferno-collapse-perf >/dev/null && command -v inferno-flamegraph >/dev/null; then
  SVG="$OUT/$NAME.svg"
  perf script -i "$DATA" 2>/dev/null | inferno-collapse-perf | inferno-flamegraph --title "$NAME" > "$SVG"
  echo "profile: $SVG (and $DATA for perf report)"
  if ((OPEN)) && command -v xdg-open >/dev/null; then xdg-open "$SVG" >/dev/null 2>&1 & fi
else
  echo "profile: $DATA (perf report -i $DATA; for an SVG: cargo install --locked inferno and rerun)"
fi
