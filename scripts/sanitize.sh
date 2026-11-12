#!/usr/bin/env bash
# Sanitizers where Miri cannot reach (docs/overhaul.md C4): the wrappers'
# ABI tests, stave-host hosting a real plugin through the full VST3 path
# (load, process, state, the simulated host), and the face runtime —
# compiled code, real threads, real allocator.
#
#   scripts/sanitize.sh address   # AddressSanitizer: out of bounds, use after free, leaks (Linux)
#   scripts/sanitize.sh thread    # ThreadSanitizer: data races
#
# Needs the pinned nightly (CI: the `sanitizers` job, nightly):
#   rustup toolchain install nightly-2026-11-01 --component rust-src
# TSan rebuilds std instrumented (-Zbuild-std), or it reports races
# inside the uninstrumented std.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
SANITIZER="${1:?usage: $0 address|thread}"
NIGHTLY="${NIGHTLY:-nightly-2026-11-01}"
TARGET="$(rustc +"$NIGHTLY" -vV | sed -n 's/^host: //p')"

export RUSTFLAGS="-Zsanitizer=${SANITIZER} -Cdebug-assertions=on"
export RUSTDOCFLAGS="$RUSTFLAGS"
export ASAN_OPTIONS="detect_leaks=$([[ "$(uname)" == Linux ]] && echo 1 || echo 0):abort_on_error=1"
export TSAN_OPTIONS="halt_on_error=1:second_deadlock_stack=1"
# The simulated host's sweep, shortened: the instrumented build is slow.
export STAVE_SIM_SEEDS="${STAVE_SIM_SEEDS:-16}"
BUILD_STD=()
[[ "$SANITIZER" == thread ]] && BUILD_STD=(-Zbuild-std)

# --target: build scripts and proc macros stay uninstrumented.
run() { cargo +"$NIGHTLY" test ${BUILD_STD[@]+"${BUILD_STD[@]}"} --target "$TARGET" --locked "$@"; }

run -p stave-vst3 --test wrapper_abi --test table_abi --test negotiation_abi --test panic_abi
run -p stave-clap --test wrapper_abi --test table_abi --test diagnostics_abi --test panic_abi --test realtime_abi
run -p stave-host --test vst3_host --test sim_host
run -p stave-face --lib --tests
run -p stave-core -p stave-telemetry --lib
