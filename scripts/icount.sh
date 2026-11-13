#!/usr/bin/env bash
# Deterministic performance regression (DSP.md P2): instructions per
# sample of each stage in crates/stave-dsp/benches/stages.rs, counted
# under valgrind's cachegrind and held to the committed baseline
# crates/stave-dsp/benches/stages.icount. Wall clock on a shared runner
# is noise; an instruction count is the same on every run.
#
#   scripts/icount.sh              # check: fail when a stage costs > +5%
#   scripts/icount.sh check biquad triode    # only these stages
#   scripts/icount.sh bless        # rewrite the baseline from this machine
#   ICOUNT_THRESHOLD=2 scripts/icount.sh     # a tighter threshold (percent)
#
# Each stage runs twice, for 16384 and 32768 samples; the difference over
# 16384 is its per-sample count, so construction, prepare and process
# start-up cancel exactly. The counts depend on the compiler, valgrind,
# glibc (libm) and the CPU family, so the baseline is blessed where CI
# runs: x86_64 Ubuntu 24.04 (glibc 2.39, valgrind 3.22) — GitHub's
# ubuntu-24.04 runner, or a Linux build host that matches it (glibc 2.39,
# valgrind 3.22):
#
#   STAVE_OFFLOAD_FETCH=crates/stave-dsp/benches/stages.icount \
#     scripts/offload.sh linux -- scripts/icount.sh bless
#
# Bless on purpose: when a change makes a stage dearer and that is the
# price of the change, or cheaper (the check reports a drop beyond the
# threshold, so the gain is locked in). Linux only (valgrind). Every run
# writes its counts to target/icount/stages.icount, which CI uploads.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
MODE="${1:-check}"
[[ $# -gt 0 ]] && shift
case "$MODE" in check | bless | measure) ;; *)
  echo "usage: $0 [check|bless|measure] [STAGE...]" >&2; exit 2 ;;
esac
BASELINE=crates/stave-dsp/benches/stages.icount
THRESHOLD="${ICOUNT_THRESHOLD:-5}"
SHORT=16384
LONG=32768
OUT="${ICOUNT_OUT:-target/icount}"
command -v valgrind >/dev/null || {
  echo "icount: needs valgrind on Linux (on a Mac: scripts/offload.sh linux -- scripts/icount.sh)" >&2
  exit 2
}

# The shipping profile (bench inherits release: fat LTO, one codegen unit).
# Its path is cargo's "Executable … (PATH)" line.
BIN="$(cargo bench --locked --color never -p stave-dsp --bench stages --no-run 2>&1 \
  | tee /dev/stderr | sed -n 's/^ *Executable .*(\(.*\))$/\1/p' | tail -1)"
[[ -x "$BIN" ]] || { echo "icount: could not build the stages bench" >&2; exit 1; }

refs() { # the instructions executed by `stages count ID N`
  valgrind --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null \
    "$BIN" count "$1" "$2" 2>&1 >/dev/null | awk '/I *refs:/ { gsub(",", "", $NF); print $NF }'
}

mkdir -p "$OUT"
ENVLINE="# $(valgrind --version) · glibc $(getconf GNU_LIBC_VERSION | cut -d" " -f2) · $(rustc --version | cut -d' ' -f1-2) · $(uname -m)"
{
  echo "# Instructions per sample of each stage in crates/stave-dsp/benches/stages.rs,"
  echo "# counted by scripts/icount.sh (cachegrind; $LONG minus $SHORT samples)."
  echo "# Bless on x86_64 Ubuntu 24.04 (CI's runner, or a matching Linux host):"
  echo "#   scripts/icount.sh bless"
  echo "$ENVLINE"
} > "$OUT/stages.icount"

STAGES=("$@")
if ((${#STAGES[@]} == 0)); then
  while IFS=$'\t' read -r id _; do STAGES+=("$id"); done < <("$BIN" list)
fi
for id in "${STAGES[@]}"; do
  a="$(refs "$id" "$SHORT")"
  b="$(refs "$id" "$LONG")"
  [[ -n "$a" && -n "$b" ]] || { echo "icount: $id did not run under valgrind" >&2; exit 1; }
  awk -v id="$id" -v a="$a" -v b="$b" -v n=$((LONG - SHORT)) 'BEGIN { printf "%s\t%.2f\n", id, (b - a) / n }' >> "$OUT/stages.icount"
done

if [[ "$MODE" == bless ]]; then
  cp "$OUT/stages.icount" "$BASELINE"
  echo "icount: blessed $BASELINE"
  grep -v '^#' "$BASELINE"
  exit 0
fi
if [[ "$MODE" == measure ]]; then
  grep -v '^#' "$OUT/stages.icount"
  exit 0
fi

[[ -f "$BASELINE" ]] || { echo "icount: no $BASELINE yet: scripts/icount.sh bless"; exit 1; }
# check: every stage measured against the baseline; with no stages
# named, the two must also hold the same stages.
base_env="$(grep '^# valgrind' "$BASELINE" || true)"
[[ "$base_env" == "$ENVLINE" ]] || echo "icount: note — measured with '${ENVLINE#\# }', baseline from '${base_env#\# }'"
all=$(($# == 0))
awk -F'\t' -v threshold="$THRESHOLD" -v all="$all" '
  /^#/ { next }
  FNR == NR { base[$1] = $2; next }
  {
    seen[$1] = 1
    if (!($1 in base)) { printf "  %-22s %10s %10.2f   NEW — not in the baseline\n", $1, "-", $2; bad = 1; next }
    d = ($2 - base[$1]) / base[$1] * 100
    flag = ""
    if (d > threshold) { flag = "   REGRESSED"; bad = 1 }
    else if (d < -threshold) { flag = "   improved (bless to lock it in)" }
    printf "  %-22s %10.2f %10.2f %+7.2f%%%s\n", $1, base[$1], $2, d, flag
  }
  END {
    if (all) for (s in base) if (!(s in seen)) { printf "  %-22s %10.2f %10s   GONE — still in the baseline\n", s, base[s], "-"; bad = 1 }
    exit bad
  }
' "$BASELINE" "$OUT/stages.icount" > "$OUT/report.txt" && status=0 || status=$?
printf "  %-22s %10s %10s %8s\n" stage baseline now change
cat "$OUT/report.txt"
if ((status)); then
  echo "icount: a stage regressed more than +$THRESHOLD% instructions/sample, or the stages and the baseline differ."
  echo "If the change is intended: scripts/icount.sh bless (on x86_64 Ubuntu 24.04) and commit $BASELINE."
  exit 1
fi
echo "icount: every stage within +$THRESHOLD% of the baseline"
