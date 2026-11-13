#!/usr/bin/env bash
# Run a command on a build host instead of this machine: the working tree
# (uncommitted changes included; anything git ignores is not sent) is synced to the
# host, the command runs there under `nice`, its output streams back, and
# its exit status is this script's.
#
#   scripts/offload.sh HOST -- cargo nextest run -p stave-dsp
#   scripts/offload.sh HOST -- scripts/gates.sh
#   STAVE_OFFLOAD_NAME=fft scripts/offload.sh HOST -- cargo clippy --workspace
#   STAVE_OFFLOAD_SYNC=tar scripts/offload.sh stave-dsp -- cargo nextest run -p stave-dsp
#   STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh HOST -- scripts/profile.sh stage biquad
#
# HOST is any ssh destination with rustup and cargo-nextest installed (the
# pinned toolchain installs itself from rust-toolchain.toml). Each tree
# gets its own directory and build cache on the host, named after the
# local directory unless STAVE_OFFLOAD_NAME says otherwise, so parallel
# agents never share a build.
#
# STAVE_OFFLOAD_FETCH names paths (relative to the tree, space-separated)
# the command writes on the host; after it exits, pass or fail, each one
# that exists is copied back into the local tree (scripts/profile.sh's
# profiles, scripts/datasheet.sh's data, target/icount).
set -euo pipefail
HOST="${1:?usage: $0 HOST -- COMMAND...}"
shift
[[ "${1:-}" == "--" ]] && shift
(($#)) || { echo "usage: $0 HOST -- COMMAND..." >&2; exit 2; }

ROOT="$(git rev-parse --show-toplevel)"
NAME="${STAVE_OFFLOAD_NAME:-$(basename "$ROOT")}"
NAME="${NAME//[^A-Za-z0-9._-]/_}"
DIR="stave-offload/$NAME"

TARGETENV=""
GITENV=""
if [[ "${STAVE_OFFLOAD_SYNC:-rsync}" == tar ]]; then
  # For hosts where rsync misbehaves (stave-dsp: the session ends with an
  # unexplained 255 after the data arrives): send exactly the files git
  # tracks or would track, compressed, unpacked fresh each time, mtimes
  # kept; the build cache lives beside the sources so it survives. Git
  # itself is not sent, so tag/history-dependent scripts belong on an
  # rsync host.
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard \
    | xargs -0 -I{} sh -c 'test -e "{}" && printf "%s\0" "{}"' \
    | tar -czf - --null -T -) \
    | ssh "$HOST" "rm -rf '$DIR' && mkdir -p '$DIR' && tar -xzf - -C '$DIR'"
  TARGETENV="CARGO_TARGET_DIR=\$HOME/$DIR.target"
else
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
  if [[ -f "$ROOT/.git" ]]; then
    COMMON="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)"
    rsync -az --delete --exclude '/worktrees/' "$COMMON/" "$HOST:$DIR.git/"
    GITENV="GIT_DIR=\$HOME/$DIR.git GIT_WORK_TREE=\$HOME/$DIR"
  fi
fi

printf -v CMD '%q ' "$@"
RUN="cd '$DIR' && . ~/.cargo/env && $GITENV $TARGETENV CARGO_TERM_COLOR=always nice -n 10 $CMD"
if [[ -z "${STAVE_OFFLOAD_FETCH:-}" ]]; then
  exec ssh "$HOST" "$RUN"
fi
status=0
ssh "$HOST" "$RUN" || status=$?
for path in $STAVE_OFFLOAD_FETCH; do
  path="${path%/}"
  if ssh "$HOST" "test -e '$DIR/$path'"; then
    mkdir -p "$ROOT/$(dirname "$path")"
    rsync -az "$HOST:$DIR/$path" "$ROOT/$(dirname "$path")/"
    echo "offload: fetched $path" >&2
  else
    echo "offload: $path was not written on $HOST" >&2
  fi
done
exit "$status"
