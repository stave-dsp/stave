#!/usr/bin/env bash
# Run a command on a build host instead of this machine: the working tree
# (uncommitted changes included; anything git ignores is not sent) is synced to the
# host, the command runs there under `nice`, its output streams back, and
# its exit status is this script's.
#
#   scripts/offload.sh HOST -- cargo nextest run -p stave-dsp
#   scripts/offload.sh HOST -- scripts/gates.sh
#   STAVE_OFFLOAD_NAME=fft scripts/offload.sh HOST -- cargo clippy --workspace
#
# HOST is any ssh destination with rustup and cargo-nextest installed (the
# pinned toolchain installs itself from rust-toolchain.toml). Each tree
# gets its own directory and build cache on the host, named after the
# local directory unless STAVE_OFFLOAD_NAME says otherwise, so parallel
# agents never share a build.
set -euo pipefail
HOST="${1:?usage: $0 HOST -- COMMAND...}"
shift
[[ "${1:-}" == "--" ]] && shift
(($#)) || { echo "usage: $0 HOST -- COMMAND..." >&2; exit 2; }

ROOT="$(git rev-parse --show-toplevel)"
NAME="${STAVE_OFFLOAD_NAME:-$(basename "$ROOT")}"
NAME="${NAME//[^A-Za-z0-9._-]/_}"
DIR="stave-offload/$NAME"

ssh "$HOST" "mkdir -p '$DIR'"
# Sources only: whatever git ignores (build output, bundles, refs) is not
# sent, and the host's own target/ (also excluded) survives --delete.
rsync -az --delete \
  --filter=':- .gitignore' \
  --exclude '/target/' --exclude '/.claude/' --exclude '**/refs/' \
  "$ROOT/" "$HOST:$DIR/"

# A git worktree's .git is a file naming a path on this machine; give the
# host the repository itself (tags and history, for the docs and release
# scripts) and point git at it there.
GITENV=""
if [[ -f "$ROOT/.git" ]]; then
  COMMON="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)"
  rsync -az --delete --exclude '/worktrees/' "$COMMON/" "$HOST:$DIR.git/"
  GITENV="GIT_DIR=\$HOME/$DIR.git GIT_WORK_TREE=\$HOME/$DIR"
fi

printf -v CMD '%q ' "$@"
exec ssh "$HOST" "cd '$DIR' && . ~/.cargo/env && $GITENV CARGO_TERM_COLOR=always nice -n 10 $CMD"
