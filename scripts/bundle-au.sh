#!/usr/bin/env bash
# Bundle a stave cdylib into an Audio Unit (.component, macOS only; does
# not build it — scripts/install.sh builds, bundles, validates and
# installs). The Info.plist's AudioComponents entry comes from the binary.
#
# Usage: scripts/bundle-au.sh <crate-name> [release|debug] [out-dir]
set -euo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
CRATE="${1:?usage: bundle-au.sh <crate-name> [release|debug] [out-dir]}"
BUNDLE=$(make_bundle "$CRATE" "${2:-release}" "${3:-build}" component)
echo "built: $BUNDLE"
