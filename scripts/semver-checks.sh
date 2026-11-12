#!/usr/bin/env bash
# The public crates' API against a released tag (docs/overhaul.md C9):
# a change the version number does not allow fails.
#
#   scripts/semver-checks.sh            # against the newest v* tag in HEAD's history
#   scripts/semver-checks.sh v0.2.1     # against a given tag
#
# The releases are git tags (nothing is on crates.io yet), so the
# baseline is a revision. A crate the baseline does not have (new since
# it) has nothing to compare against and is skipped, by name. CI runs
# this on every push to a release/* branch, where a patch release must
# not break its API; scripts/release.sh runs it before tagging. Tool:
# cargo-semver-checks 0.51.0.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BASE="${1:-$(git describe --tags --abbrev=0 --match 'v*' HEAD)}"
# The tools, the example, the test harness and the guide are not a
# published API.
EXCLUDE='^(stave-bench|stave-play|stave-storybook|stave-tilt-vst3|stave-test|stave-guide|stave-docsite)$'
PACKAGES=()
while read -r name manifest; do
  [[ "$name" =~ $EXCLUDE ]] && continue
  rel="${manifest#"$ROOT"/}"
  if git cat-file -e "${BASE}:${rel}" 2>/dev/null; then
    PACKAGES+=(--package "$name")
  else
    echo "semver-checks: ${name} is new since ${BASE}, skipped"
  fi
done < <(cargo metadata --no-deps --format-version 1 --locked |
  python3 -c 'import json,sys; [print(p["name"], p["manifest_path"]) for p in json.load(sys.stdin)["packages"]]')
echo "semver-checks against ${BASE}: ${#PACKAGES[@]} args"
cargo semver-checks check-release --baseline-rev "$BASE" "${PACKAGES[@]}"
