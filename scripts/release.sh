#!/usr/bin/env bash
# Cut a framework release that stave-plugins (and anyone else) can pin.
#
#   scripts/release.sh 0.2.0
#
# Requires a clean tree on main. Sets [workspace.package] version (every
# crate inherits it), refreshes Cargo.lock, checks the version against
# the API change (scripts/semver-checks.sh), runs the gates, commits, tags
# v<version> and pushes main + the tag to every configured remote.
# Then, in stave-plugins: scripts/bump-framework.sh v<version>.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="${1:?usage: $0 <version, e.g. 0.2.0>}"
TAG="v${VERSION}"

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be MAJOR.MINOR.PATCH" >&2; exit 1; }
[[ "$(git branch --show-current)" == main ]] || { echo "release from main" >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo "working tree is not clean" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null && { echo "${TAG} already exists" >&2; exit 1; }

python3 - "$VERSION" <<'PY'
import re, sys
s = open("Cargo.toml").read()
s, n = re.subn(r'(?m)^(\[workspace\.package\]\n(?:.*\n)*?version = )"[^"]+"', r'\1"%s"' % sys.argv[1], s, count=1)
assert n == 1, "no version in [workspace.package]"
open("Cargo.toml", "w").write(s)
PY
cargo update --workspace
# The version must allow the API change since the last release.
bash scripts/semver-checks.sh "$(git describe --tags --abbrev=0 --match 'v*')"
bash scripts/gates.sh

git commit -qam "Release ${TAG}"
git tag -a "$TAG" -m "stave ${VERSION}"
for remote in $(git remote); do
  git push "$remote" main "$TAG"
done
echo "released ${TAG}; next, in stave-plugins: scripts/bump-framework.sh ${TAG}"
