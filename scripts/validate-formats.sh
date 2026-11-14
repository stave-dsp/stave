#!/usr/bin/env bash
# Build every plugin in release, bundle it, and run the formats' own
# validators over the bundles: Steinberg's validator for VST3,
# clap-validator for CLAP, Apple's auval for Audio Units (macOS).
#
# Usage:
#   scripts/validate-formats.sh [vst3|clap|au|all] [out-dir]
#
# VALIDATOR overrides the Steinberg validator's path (default: the SDK
# build under target/vst3sdk, as CI and a local build lay it out).
#
# `au` installs each .component into ~/Library/Audio/Plug-Ins/Components
# (auval finds only registered components) and runs `auval -v` with the
# codes from its Info.plist, keeping its whole report beside the bundle
# (<Name>.auval.txt); it is part of `all` on macOS only.
set -euo pipefail

FORMATS="${1:-all}"
source "$(dirname "$0")/lib/bundle-common.sh"
OUT_DIR="${2:-${REPO_ROOT}/target/validate}"
VALIDATOR=$(vst3_validator)

case "$FORMATS" in vst3 | clap | au | all) ;; *) echo "usage: $0 [vst3|clap|au|all] [out-dir]" >&2; exit 2 ;; esac
[[ "$FORMATS" == au && "$STAVE_OS" != macos ]] && { echo "Audio Units are a macOS format" >&2; exit 2; }

crates=()
while IFS= read -r c; do crates+=("$c"); done < <(plugin_crates)
build_args=()
for c in "${crates[@]}"; do build_args+=(-p "$c"); done
cargo build --release "${build_args[@]}"

rm -rf "$OUT_DIR"
fail=0
if [[ "$FORMATS" == vst3 || "$FORMATS" == all ]]; then
  [[ -x "$VALIDATOR" ]] || { echo "Steinberg validator not found (set VALIDATOR)" >&2; exit 2; }
  for c in "${crates[@]}"; do make_bundle "$c" release "$OUT_DIR" vst3 >/dev/null; done
  for b in "$OUT_DIR"/*.vst3; do
    result=$("$VALIDATOR" "$b" 2>&1 | grep 'Result:' | tail -1 || true)
    # (a failing run prints its failing tests with the rest of its log)
    echo "$(basename "$b"): ${result:-no result}"
    [[ "$result" == *" 0 tests failed"* ]] || fail=1
  done
fi
if [[ "$FORMATS" == clap || "$FORMATS" == all ]]; then
  command -v clap-validator >/dev/null || { echo "clap-validator not on PATH" >&2; exit 2; }
  for c in "${crates[@]}"; do make_bundle "$c" release "$OUT_DIR" clap >/dev/null; done
  for b in "$OUT_DIR"/*.clap; do
    # clap-validator exits non-zero when a test fails; keep going and
    # show the failures instead of dying silently under pipefail.
    out=$(clap-validator validate "$b" 2>&1 | sed $'s/\x1b\\[[0-9;]*m//g' || true)
    result=$(printf '%s\n' "$out" | tail -1)
    echo "$(basename "$b"): $result"
    if [[ "$result" != *" 0 failed"* ]]; then
      printf '%s\n' "$out" | grep -iE 'failed|error|warning' | head -40
      fail=1
    fi
  done
fi
if [[ "$FORMATS" == au || ( "$FORMATS" == all && "$STAVE_OS" == macos ) ]]; then
  for c in "${crates[@]}"; do make_bundle "$c" release "$OUT_DIR" component >/dev/null; done
  for b in "$OUT_DIR"/*.component; do
    fresh_install "$b" "$USER_AU_DIR"
    codes=()
    for key in type subtype manufacturer; do
      codes+=("$(plutil -extract "AudioComponents.0.$key" raw -o - "$b/Contents/Info.plist")")
    done
    # The registrar picks a new component up within seconds; auval says
    # "didn't find the component" until it has.
    out=""
    for _ in 1 2 3 4 5 6; do
      out=$(auval -v "${codes[@]}" 2>&1 || true)
      [[ "$out" == *"didn't find the component"* ]] || break
      sleep 5
    done
    printf '%s\n' "$out" > "${b%.component}.auval.txt"
    result=$(printf '%s\n' "$out" | grep -E 'AU VALIDATION (SUCCEEDED|FAILED)' | tail -1 || true)
    echo "$(basename "$b") (${codes[*]}): ${result:-no result}"
    if [[ "$result" != *SUCCEEDED* ]]; then
      printf '%s\n' "$out" | grep -iE 'error|fail|warning' | head -40
      fail=1
    fi
  done
fi
exit "$fail"
