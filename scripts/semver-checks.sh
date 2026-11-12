#!/usr/bin/env bash
# The public crates' API against a released tag (docs/overhaul.md C9):
# a change the version number does not allow fails.
#
#   scripts/semver-checks.sh            # against the newest v* tag in HEAD's history
#   scripts/semver-checks.sh v0.2.1     # against a given tag
#
# The releases are git tags (nothing is on crates.io yet), so the
# baseline is a revision. CI runs this on every push to a release/*
# branch, where a patch release must not break its API; scripts/release.sh
# runs it before tagging. Tool: cargo-semver-checks 0.51.0.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BASE="${1:-$(git describe --tags --abbrev=0 --match 'v*' HEAD)}"
echo "semver-checks against ${BASE}"
# The tools, the example and the test harness are not a published API.
cargo semver-checks check-release --workspace --baseline-rev "$BASE" \
  --exclude stave-bench --exclude stave-play --exclude stave-storybook \
  --exclude stave-tilt-vst3 --exclude stave-test
