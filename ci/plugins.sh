#!/usr/bin/env bash
# ci.yml's `plugins` job for ci/land.yaml: do the plugins in
# stave-dsp/stave-plugins still build and lint against this checkout?
# The stave-plugins branch is scripts/plugins-ref.sh's choice from the
# commit's message (a `plugins-branch:` line) and the release being
# prepared, else main; a landing has no pull request, so the rule that
# matches a pull request's branch name never applies. The clone is made
# fresh in a temporary directory and removed afterwards.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
git log -1 --format=%B > "$work/change.txt"
choice="$(scripts/plugins-ref.sh --text "$work/change.txt")"
echo "$choice"
ref="$(sed -n 's/^PLUGINS_REF=//p' <<<"$choice")"

git clone -q --depth 1 --branch "$ref" https://github.com/stave-dsp/stave-plugins.git "$work/stave-plugins"
cd "$work/stave-plugins"
echo "stave-plugins $(git rev-parse HEAD) ($ref)"
scripts/link-framework.sh "$ROOT"
cargo clippy --workspace --all-targets -- -D warnings
