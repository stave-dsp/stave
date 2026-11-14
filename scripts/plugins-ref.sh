#!/usr/bin/env bash
# Which stave-plugins branch a framework change is built against (ci.yml's
# `plugins` job). A planned API break leaves stave-plugins' main behind the
# framework's main until the next release; a branch of stave-plugins that
# follows the break keeps the job meaningful. The first that applies:
#
#   1. a `plugins-branch: <name>` line in the change's description (the
#      pull request's body, or the head commit's message) — that branch,
#      which must exist;
#   2. a stave-plugins branch with the same name as the pull request's
#      branch;
#   3. a stave-plugins branch `stave-<next>` or `stave-<next major.minor>`,
#      where <next> is the newest releases/v<next>.md beyond the workspace
#      version (the release being prepared, e.g. stave-0.5.0 or stave-0.5);
#   4. main.
#
#   scripts/plugins-ref.sh [--text FILE] [--branch NAME] [--repo URL]
#
# Prints `PLUGINS_REF=<branch>` and `PLUGINS_REF_WHY=<reason>` (for
# $GITHUB_ENV); fails only when a named `plugins-branch:` does not exist.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

text="" branch="" repo="https://github.com/stave-dsp/stave-plugins.git"
while (($#)); do
  case "$1" in
    --text) text="$(cat "${2:?--text needs a file}")"; shift ;;
    --branch) branch="${2-}"; shift ;;
    --repo) repo="${2:?--repo needs a URL}"; shift ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "plugins-ref.sh: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

heads="$(git ls-remote --heads "$repo" | sed 's#.*refs/heads/##')"
exists() { grep -qxF "$1" <<<"$heads"; }
pick() { printf 'PLUGINS_REF=%s\nPLUGINS_REF_WHY=%s\n' "$1" "$2"; exit 0; }

named="$(sed -n 's/^[[:space:]]*plugins-branch:[[:space:]]*\([^[:space:]]*\).*/\1/p' <<<"$text" | tail -1)"
if [[ -n "$named" ]]; then
  exists "$named" || { echo "plugins-ref.sh: plugins-branch: $named does not exist in $repo" >&2; exit 1; }
  pick "$named" "named by plugins-branch:"
fi
if [[ -n "$branch" ]] && [[ "$branch" != main ]] && exists "$branch"; then
  pick "$branch" "the same branch name as this change"
fi
current="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\([0-9.]*\)".*/\1/p' Cargo.toml)"
next="$(ls releases 2>/dev/null | sed -n 's/^v\([0-9]*\.[0-9]*\.[0-9]*\)\.md$/\1/p' \
  | { cat; echo "$current"; } | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)"
if [[ -n "$next" && "$next" != "$current" ]]; then
  for b in "stave-$next" "stave-${next%.*}"; do
    exists "$b" && pick "$b" "the branch preparing for v$next"
  done
fi
pick main "no branch follows this change"
