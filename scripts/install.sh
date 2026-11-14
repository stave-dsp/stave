#!/usr/bin/env bash
# Build → bundle → validate → fresh-inode install, one command.
# Exists because of two footguns: bundling without building (a stale
# dylib once passed validation), and cp -R over a bundle a running host
# has mapped (see fresh_install in lib/bundle-common.sh).
#
# Usage: scripts/install.sh <crate-name> [release|debug]
#
# Validators run when available: VALIDATOR (or target/vst3sdk, see
# scripts/validate-formats.sh) for VST3, clap-validator on PATH for CLAP;
# on macOS the Audio Unit is installed too, then checked by auval.
set -euo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
CRATE="${1:?usage: install.sh <crate-name> [release|debug]}"
PROFILE="${2:-release}"
cd "$REPO_ROOT"

# shellcheck disable=SC2046
cargo build $(profile_args "$PROFILE") -p "$CRATE"
VST3=$(make_bundle "$CRATE" "$PROFILE" build vst3)
CLAP=$(make_bundle "$CRATE" "$PROFILE" build clap)

validator=$(vst3_validator)
if [[ -x "$validator" ]]; then
  "$validator" "$VST3" 2>&1 | grep "Result:" || { echo "Steinberg validation FAILED" >&2; exit 1; }
else
  echo "note: Steinberg validator not found — set VALIDATOR (skipping)"
fi
if command -v clap-validator >/dev/null; then
  clap-validator validate "$CLAP" 2>&1 | tail -1 | grep -q " 0 failed" || {
    echo "clap-validator FAILED" >&2; exit 1; }
else
  echo "note: clap-validator not on PATH (skipping)"
fi

fresh_install "$VST3" "$USER_VST3_DIR"
fresh_install "$CLAP" "$USER_CLAP_DIR"
formats="VST3 + CLAP"
if [[ $STAVE_OS == macos ]]; then
  AU=$(make_bundle "$CRATE" "$PROFILE" build component)
  fresh_install "$AU" "$USER_AU_DIR"
  formats="VST3 + CLAP + AU"
  codes=()
  for key in type subtype manufacturer; do
    codes+=("$(plutil -extract "AudioComponents.0.$key" raw -o - "$AU/Contents/Info.plist")")
  done
  # auval sees the component once the registrar has (seconds).
  sleep 3
  auval -v "${codes[@]}" 2>&1 | grep -E "AU VALIDATION (SUCCEEDED|FAILED)" | grep -q SUCCEEDED || {
    echo "auval FAILED (auval -v ${codes[*]})" >&2; exit 1; }
fi
echo "installed $(basename "$VST3" .vst3) ($formats, fresh inodes)"
