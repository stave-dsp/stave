#!/usr/bin/env bash
# Bundle a stave cdylib into a .clap for this OS (does not build it;
# scripts/install.sh builds, bundles, validates and installs).
#
# Usage: scripts/bundle-clap.sh <crate-name> [release|debug] [out-dir]
set -euo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
CRATE="${1:?usage: bundle-clap.sh <crate-name> [release|debug] [out-dir]}"
BUNDLE=$(make_bundle "$CRATE" "${2:-release}" "${3:-build}" clap)
echo "built: $BUNDLE"
