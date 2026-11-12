#!/usr/bin/env bash
# Bundle a stave cdylib into a .vst3 bundle for this OS (does not build it;
# scripts/install.sh builds, bundles, validates and installs).
#
# Usage: scripts/bundle-vst3.sh <crate-name> [release|debug] [out-dir]
#
# Produces <out-dir>/<Name>.vst3: Contents/{Info.plist,PkgInfo,MacOS/<Name>}
# on macOS, Contents/<arch>-linux/<Name>.so on Linux,
# Contents/<arch>-win/<Name>.vst3 on Windows (Git Bash).
set -euo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
CRATE="${1:?usage: bundle-vst3.sh <crate-name> [release|debug] [out-dir]}"
BUNDLE=$(make_bundle "$CRATE" "${2:-release}" "${3:-build}" vst3)
echo "built: $BUNDLE"
