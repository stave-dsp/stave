#!/usr/bin/env bash
# Build, bundle (VST3 + CLAP, and on macOS the Audio Unit) and
# fresh-install every plugin in examples/.
# One failing plugin doesn't stop the batch; the exit code reports it.
# For validation as well, use scripts/validate-formats.sh (all plugins)
# or scripts/install.sh (one plugin).
#
# Usage: scripts/bundle-all.sh [release|debug]
set -uo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
PROFILE="${1:-release}"
cd "$REPO_ROOT"

crates=()
while IFS= read -r c; do crates+=("$c"); done < <(plugin_crates)

echo "==> Building ${#crates[@]} plugin(s)"
fail=0
build_args=()
for c in "${crates[@]}"; do build_args+=(-p "$c"); done
# shellcheck disable=SC2046
cargo build $(profile_args "$PROFILE") "${build_args[@]}" || { echo "  ! build failed (bundling what built)"; fail=1; }

echo "==> Bundling + installing"
for c in "${crates[@]}"; do
  formats=(vst3 clap)
  [[ $STAVE_OS == macos ]] && formats+=(component)
  for ext in "${formats[@]}"; do
    if bundle=$(make_bundle "$c" "$PROFILE" build "$ext"); then
      case $ext in vst3) dest=$USER_VST3_DIR ;; clap) dest=$USER_CLAP_DIR ;; component) dest=$USER_AU_DIR ;; esac
      fresh_install "$bundle" "$dest"
      echo "  installed $(basename "$bundle")"
    else
      echo "  ! bundle failed: $c ($ext)"; fail=1
    fi
  done
done
exit "$fail"
