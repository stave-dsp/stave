#!/usr/bin/env bash
# Run a command on a build host instead of this machine: the working tree
# (uncommitted changes included; anything git ignores is not sent) is synced
# to the host, the command runs there under `nice`, its output streams back,
# and its exit status is this script's.
#
#   scripts/offload.sh linux -- cargo nextest run -p stave-dsp
#   scripts/offload.sh macos -- scripts/gates.sh
#   scripts/offload.sh some-ssh-host -- cargo clippy --workspace
#   STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh macos -- scripts/profile.sh stage biquad
#
# HOST is `linux` or `macos` (the hosts named by STAVE_LINUX_HOST and
# STAVE_MACOS_HOST in your local configuration — see stave.env.example),
# `auto` (whichever of those two is less loaded per core), or any ssh
# destination. The host needs rustup and cargo-nextest; the pinned
# toolchain installs itself from rust-toolchain.toml. Each tree gets its own
# directory and build cache on the host, named after the local directory
# unless STAVE_OFFLOAD_NAME says otherwise, so parallel builds never share
# one.
#
# Hosts listed in STAVE_TAR_HOSTS (or any host with STAVE_OFFLOAD_SYNC=tar)
# get a tarball instead of rsync. STAVE_OFFLOAD_FETCH names paths (relative
# to the tree, space-separated) the command writes on the host; after it
# exits, pass or fail, each one that exists is copied back into the local
# tree (scripts/profile.sh's profiles, scripts/datasheet.sh's data).
set -euo pipefail
HOST="${1:?usage: $0 HOST -- COMMAND...}"
shift
[[ "${1:-}" == "--" ]] && shift
(($#)) || { echo "usage: $0 HOST -- COMMAND..." >&2; exit 2; }

source "$(dirname "$0")/lib/config.sh"
stave_config_load "$(git rev-parse --show-toplevel)"
# Load per core on a host (one-minute average), for `auto`; `local` is
# this machine.
load_per_core() {
  local probe='l=$(uptime | sed -E "s/.*load averages?: *([0-9.]+).*/\1/");
    n=$(nproc 2>/dev/null || sysctl -n hw.ncpu); awk -v l="$l" -v n="$n" "BEGIN{printf \"%.3f\", l/n}"'
  if [[ "$1" == local ]]; then
    # Local builds fill this disk (a target directory per checkout): leave
    # the pool below STAVE_AUTO_MIN_FREE_GB free (default 30).
    local free_gb
    free_gb="$(df -Pk "$(git rev-parse --show-toplevel)" | awk 'NR==2{printf "%d", $4/1048576}')"
    if ((free_gb < ${STAVE_AUTO_MIN_FREE_GB:-30})); then echo 999; return; fi
    bash -c "$probe" 2>/dev/null || echo 999
  else ssh "$1" "$probe" 2>/dev/null || echo 999; fi
}
case "$HOST" in
  linux) HOST="$(stave_config_require STAVE_LINUX_HOST "offload.sh linux")" ;;
  macos) HOST="$(stave_config_require STAVE_MACOS_HOST "offload.sh macos")" ;;
  auto)
    # The least loaded (per core) of STAVE_AUTO_HOSTS — by default the
    # configured Linux and macOS hosts; `local` adds this machine. When
    # every host is above STAVE_AUTO_MAX_LOAD per core (default 1.5) the
    # command waits for one to free up rather than piling on, so many
    # parallel builds queue briefly instead of all slowing down.
    POOL="${STAVE_AUTO_HOSTS:-${STAVE_LINUX_HOST:-} ${STAVE_MACOS_HOST:-}}"
    [[ -n "${POOL// /}" ]] || { echo "offload.sh auto needs STAVE_AUTO_HOSTS or the linux/macos hosts (stave.env.example)" >&2; exit 2; }
    MAX="${STAVE_AUTO_MAX_LOAD:-1.5}"
    waited=0
    while :; do
      best="" best_load=999
      for h in $POOL; do
        l="$(load_per_core "$h")"
        if awk -v a="$l" -v b="$best_load" 'BEGIN{exit !(a < b)}'; then best="$h"; best_load="$l"; fi
      done
      if awk -v a="$best_load" -v m="$MAX" 'BEGIN{exit !(a <= m)}' || ((waited >= 1800)); then break; fi
      ((waited == 0)) && echo "offload: every host is above $MAX load per core; waiting" >&2
      sleep 15; waited=$((waited + 15))
    done
    HOST="$best"
    echo "offload: auto chose $HOST (load $best_load per core, waited ${waited}s)" >&2
    ;;
esac
if [[ "$HOST" == local ]]; then
  # This machine: no sync, its own tree and target directory.
  cd "$(git rev-parse --show-toplevel)" && exec nice -n 10 "$@"
fi
SYNC="${STAVE_OFFLOAD_SYNC:-rsync}"
for tar_host in ${STAVE_TAR_HOSTS:-}; do
  [[ "$tar_host" == "$HOST" ]] && SYNC="${STAVE_OFFLOAD_SYNC:-tar}"
done

ROOT="$(git rev-parse --show-toplevel)"
NAME="${STAVE_OFFLOAD_NAME:-$(basename "$ROOT")}"
NAME="${NAME//[^A-Za-z0-9._-]/_}"
DIR="stave-offload/$NAME"

TARGETENV=""
GITENV=""
if [[ "$SYNC" == tar ]]; then
  # For hosts where rsync misbehaves (seen behind an ssh relay into a
  # container: the session ends with an unexplained 255 after the data
  # arrives): send exactly the files git
  # tracks or would track, compressed, unpacked fresh each time, mtimes
  # kept; the build cache lives beside the sources so it survives. The
  # host gets a fresh index of exactly those files (no history), so the
  # git-grep tree checks (vocabulary, purity) check the real tree; tag- and
  # history-dependent scripts belong on an rsync host.
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard \
    | xargs -0 -I{} sh -c 'test -e "{}" && printf "%s\0" "{}"' \
    | COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata -czf - --null -T -) \
    > "${TMPDIR:-/tmp}/offload-$NAME.$$.tgz"
  TARBALL="${TMPDIR:-/tmp}/offload-$NAME.$$.tgz"
  trap 'rm -f "$TARBALL"' EXIT
  TARGETENV="CARGO_TARGET_DIR=\$HOME/$DIR.target"
else
  ssh "$HOST" "mkdir -p '$DIR'"
  # Sources only: whatever git ignores (build output, bundles, refs) is not
  # sent, and the host's own target/ (also excluded) survives --delete.
  # .git goes whole (rules come first-match-first: gitignore's **/refs/
  # must not reach .git/refs).
  rsync -az --delete \
    --include '/.git/***' \
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
status=0
if [[ "$SYNC" == tar ]]; then
  # Unpack and run under one lock per name: a second command with the same
  # STAVE_OFFLOAD_NAME waits instead of unpacking over a running build.
  ssh "$HOST" "mkdir -p stave-offload && flock -w 3600 '$DIR.lock' bash -c \"rm -rf '$DIR' && mkdir -p '$DIR' && tar -xzf - -C '$DIR' && git -C '$DIR' init -q && git -C '$DIR' add -A && $RUN\"" \
    < "$TARBALL" || status=$?
elif [[ -z "${STAVE_OFFLOAD_FETCH:-}" ]]; then
  exec ssh "$HOST" "$RUN"
else
  ssh "$HOST" "$RUN" || status=$?
fi
[[ -z "${STAVE_OFFLOAD_FETCH:-}" ]] && exit "$status"
for path in $STAVE_OFFLOAD_FETCH; do
  path="${path%/}"
  if ssh "$HOST" "test -e '$DIR/$path'"; then
    mkdir -p "$ROOT/$(dirname "$path")"
    if [[ "$SYNC" == tar ]]; then
      ssh "$HOST" "tar -czf - -C '$DIR' '$path'" | tar -xzf - -C "$ROOT"
    else
      rsync -az "$HOST:$DIR/$path" "$ROOT/$(dirname "$path")/"
    fi
    echo "offload: fetched $path" >&2
  else
    echo "offload: $path was not written on $HOST" >&2
  fi
done
exit "$status"
