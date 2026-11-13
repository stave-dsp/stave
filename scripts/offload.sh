#!/usr/bin/env bash
# Run a command on a build host instead of this machine: the working tree
# (uncommitted changes included; anything git ignores is not sent) is synced
# to one of the host's workspace slots, the command runs there under `nice`
# with that slot's share of the cores, its output streams back, and its exit
# status is this script's.
#
#   scripts/offload.sh linux -- cargo nextest run -p stave-dsp
#   scripts/offload.sh macos -- scripts/gates.sh
#   scripts/offload.sh auto -- cargo clippy --workspace
#   STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh macos -- scripts/profile.sh stage biquad
#   STAVE_OFFLOAD_CLASS=long scripts/offload.sh linux -- scripts/kani.sh
#   scripts/offload.sh gc linux [--dry-run]   # remove pre-slot per-name workspaces
#   scripts/offload.sh stats [HOURS]          # utilisation, waits, run times
#
# HOST is `linux` or `macos` (the hosts named by STAVE_LINUX_HOST and
# STAVE_MACOS_HOST in your local configuration — see stave.env.example),
# `auto` (a free slot on any of STAVE_AUTO_HOSTS, the least loaded first),
# `local` (this machine, in this checkout) or any ssh destination; `HOST:N`
# gives it N build slots. The host needs rustup, cargo-nextest and perl; the
# pinned toolchain installs itself from rust-toolchain.toml.
#
# Slots. A host runs at most its slot count of jobs at once (default: a
# quarter of its cores, at least one; `local`: one); a job waits, saying so,
# until a slot is free. Each slot keeps one workspace and its build cache,
# synced by content so unchanged files keep their mtimes and builds stay
# incremental whichever checkout sent the last job. A job's cargo, nextest,
# libtest, rayon and OpenMP get the slot's share of the cores
# (cores / slots). STAVE_OFFLOAD_NAME labels the job (queue messages, the
# log); it no longer picks a directory. STAVE_OFFLOAD_CLASS=long (ngspice
# batches, fuzzing, kani, datasheet fits) runs at nice 19, one per host, in
# the host's own long slot (STAVE_OFFLOAD_LONG_SLOT, default 1 on ssh hosts)
# so it never holds a build slot.
#
# The command runs in its own process group on the host. When this script
# ends — finished, killed, or its ssh session dropped — whatever the command
# left running there is stopped. STAVE_OFFLOAD_TIMEOUT (seconds, or 30m, 2h)
# stops the job after that long, saying so, with exit status 124; unset, a
# job runs as long as it needs.
#
# Hosts listed in STAVE_TAR_HOSTS (or any host with STAVE_OFFLOAD_SYNC=tar)
# get a tarball instead of rsync, with a fresh git index of exactly the files
# sent (the vocabulary and purity checks read `git ls-files`).
# STAVE_OFFLOAD_FETCH names paths (relative to the tree, space-separated) the
# command writes on the host; after it exits, pass or fail, each one that
# exists is copied back into the local tree.
#
# Every job appends a line to ${XDG_STATE_HOME:-~/.local/state}/stave/offload.jsonl
# (host, slot, queue wait, sync and run seconds, exit status, timeout);
# `stats` summarises it.
set -euo pipefail
usage() { echo "usage: $0 HOST -- COMMAND...   |   $0 gc HOST [--dry-run]   |   $0 stats [HOURS]" >&2; exit 2; }
HOST="${1:-}"
[[ -n "$HOST" ]] || usage
shift

LIB="$(cd "$(dirname "$0")/lib" && pwd)"
LOG="${XDG_STATE_HOME:-$HOME/.local/state}/stave/offload.jsonl"

if [[ "$HOST" == stats ]]; then
  [[ -f "$LOG" ]] || { echo "offload: no jobs logged yet ($LOG)" >&2; exit 0; }
  exec perl "$LIB/offload-stats.pl" "${1:-24}" "$LOG"
fi

ROOT="$(git rev-parse --show-toplevel)"
source "$LIB/config.sh"
stave_config_load "$ROOT"
RUNNER="$(base64 < "$LIB/offload-runner.pl" | tr -d '\n')"

GC=0
if [[ "$HOST" == gc ]]; then
  GC=1
  HOST="${1:-}"
  [[ -n "$HOST" ]] || usage
  shift
else
  [[ "${1:-}" == "--" ]] && shift
  (($#)) || usage
fi

# `linux`/`macos` → the configured host. An optional `:N` is a slot count.
resolve() {
  case "$1" in
    linux) stave_config_require STAVE_LINUX_HOST "offload.sh linux" ;;
    macos) stave_config_require STAVE_MACOS_HOST "offload.sh macos" ;;
    *) printf '%s' "$1" ;;
  esac
}
# Build slots for a host: HOST:N, else its entry in STAVE_AUTO_HOSTS, else
# the per-role key; empty lets the host pick (a quarter of its cores).
slots_for() {
  local h="$1" entry
  [[ "$h" == *:* ]] && { printf '%s' "${h##*:}"; return; }
  for entry in ${STAVE_AUTO_HOSTS:-}; do
    [[ "$entry" == "$h":* ]] && { printf '%s' "${entry##*:}"; return; }
  done
  if [[ "$h" == local ]]; then printf '%s' "${STAVE_LOCAL_SLOTS:-1}"
  elif [[ "$h" == "${STAVE_LINUX_HOST:-}" ]]; then printf '%s' "${STAVE_LINUX_SLOTS:-}"
  elif [[ "$h" == "${STAVE_MACOS_HOST:-}" ]]; then printf '%s' "${STAVE_MACOS_SLOTS:-}"
  fi
}
# Where the slots live: in the home directory on a host; for `local`, only
# locks (the command runs in this checkout).
slot_root() {
  if [[ "$1" == local ]]; then printf '%s' "${XDG_CACHE_HOME:-$HOME/.cache}/stave-offload/slots"
  else printf '%s' stave-offload/slots; fi
}
long_slot() {
  if [[ "$1" == local ]]; then printf 0; else printf '%s' "${STAVE_OFFLOAD_LONG_SLOT:-1}"; fi
}

# Run the host-side runner (scripts/lib/offload-runner.pl) in MODE on HOST.
# Arguments go base64-encoded so no shell on the way can reinterpret them.
# It replaces the calling shell: call it in a subshell, or last.
runner() {
  local host="$1" mode="$2" a args=""
  shift 2
  for a in "$@"; do args+=" $(printf 'x%s' "$a" | base64 | tr -d '\n')"; done
  local boot='eval decode_base64(shift); die $@ if $@'
  if [[ "$host" == local ]]; then
    # shellcheck disable=SC2086
    exec perl -MMIME::Base64 -e "$boot" "$RUNNER" "$mode" $args
  else
    exec ssh "$host" "exec perl -MMIME::Base64 -e '$boot' $RUNNER $mode$args"
  fi
}

if ((GC)); then
  HOST="$(resolve "$HOST")"
  HOST="${HOST%%:*}"
  [[ "$HOST" != local ]] || { echo "offload: gc is for build hosts" >&2; exit 2; }
  runner "$HOST" gc stave-offload "$([[ "${1:-}" == --dry-run ]] && echo 1)"
  exit
fi

CLASS="${STAVE_OFFLOAD_CLASS:-build}"
case "$CLASS" in build | long) ;; *) echo "offload: STAVE_OFFLOAD_CLASS is build or long, not $CLASS" >&2; exit 2 ;; esac
NAME="${STAVE_OFFLOAD_NAME:-$(basename "$ROOT")}"
NAME="${NAME//[^A-Za-z0-9._-]/_}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/offload.XXXXXX")"
exec 5<&0   # the command's stdin (background jobs would get /dev/null)
HOLD_PID="" JOB_PID="" SLOT="" RESULT="" LOGGED=0
T0="$(date +%s)" T_SLOT="" T_SYNC="" T_END=""

# STAVE_OFFLOAD_EVENTS, if set, is a command that receives one JSON line on
# stdin when a job takes its slot and when it ends (e.g. a dashboard's
# ingest CLI). It runs in the background and its failure never fails a job.
event() { # kind [outcome: ok|failed] [why]
  [[ -n "${STAVE_OFFLOAD_EVENTS:-}" ]] || return 0
  perl -MJSON::PP -e '
    my ($kind, $label, $host, $class, $slot, $t0, $pid, $outcome, $why) = @ARGV;
    my %e = (kind => $kind, project => "stave", id => "offload-$t0-$pid-$label",
      title => $label, host => $host, slot => $slot, detail => length $why ? "$class slot, $why" : "$class slot", at => time() * 1000);
    $e{outcome} = $outcome if length $outcome;
    print JSON::PP->new->canonical->encode(\%e), "\n";
  ' "$1" "$NAME" "$HOST" "$CLASS" "$SLOT" "$T0" "$$" "${2:-}" "${3:-}" \
    | bash -c "$STAVE_OFFLOAD_EVENTS" >/dev/null 2>&1 &
  disown $! 2>/dev/null || true
}

log_job() {
  ((LOGGED)) && return 0
  LOGGED=1
  [[ -n "$SLOT" ]] || return 0
  local st="${STATUS:-1}"
  [[ "$RESULT" == *"exit "* ]] && st="${RESULT#*exit }" && st="${st%% *}"
  event job.finished "$([[ "$st" == 0 ]] && echo ok || echo failed)" "exit $st"
  mkdir -p "$(dirname "$LOG")"
  T_END="${T_END:-$(date +%s)}"
  T_SLOT="${T_SLOT:-$T_END}"
  STAVE_LOG_RESULT="$RESULT" perl -MJSON::PP -e '
    my %r = split " ", ($ENV{STAVE_LOG_RESULT} // "");
    my ($label, $host, $class, $slot, $slots, $jobs, $t0, $tslot, $tsync, $tend, $status, @cmd) = @ARGV;
    $tsync ||= $tend;
    my %j = (end => $tend + 0, label => $label, host => $host, class => $class, slot => $slot,
      slots => $slots + 0, jobs => $jobs + 0, wait_s => $tslot - $t0, sync_s => $tsync - $tslot,
      run_s => $tend - $tsync, exit => (exists $r{exit} ? $r{exit} : $status) + 0,
      timed_out => ($r{timed_out} // 0) ? JSON::PP::true : JSON::PP::false,
      command => join(" ", @cmd));
    print JSON::PP->new->canonical->encode(\%j), "\n";
  ' "$NAME" "$HOST" "$CLASS" "$SLOT" "$NSLOTS" "$JOBS" "$T0" "$T_SLOT" "${T_SYNC:-}" "$T_END" "${STATUS:-1}" "$@" >> "$LOG" || true
}
# Give the slot back: closing the holder's stdin ends it; it stops anything
# still running in the slot and reports the job's result.
release() {
  [[ -n "$HOLD_PID" ]] || return 0
  exec 3>&-
  IFS= read -r line <&4 && [[ "$line" == result\ * ]] && RESULT="${line#result }"
  exec 4<&-
  wait "$HOLD_PID" 2>/dev/null || true
  HOLD_PID=""
}
cleanup() {
  local s=$?
  STATUS="${STATUS:-$s}"
  if [[ -n "$JOB_PID" ]]; then kill "$JOB_PID" 2>/dev/null || true; fi
  release
  log_job "$@"
  rm -rf "$WORK"
}
trap 'cleanup "$@"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

# Run a step in the background and wait for it, so a signal to this script
# is handled at once instead of after the step (bash defers traps until a
# foreground command ends). Its stdin is ours.
step() {
  local s=0
  "$@" <&0 &
  JOB_PID=$!
  wait "$JOB_PID" || s=$?
  JOB_PID=""
  return "$s"
}

# Try to take a free slot on HOST. Sets SLOT, NSLOTS, CORES, JOBS and leaves
# the holder running; returns 1 when every slot is busy (BUSY says by
# whom), 2 when the host could not be reached. Waiting happens here, not in
# an ssh session per queued job (that would exhaust the host's sshd
# MaxSessions).
take_slot() {
  local host="$1" want="$2" line=""
  rm -f "$WORK/ctl" "$WORK/out"
  mkfifo "$WORK/ctl" "$WORK/out"
  runner "$host" hold "$(slot_root "$host")" "$want" "$(long_slot "$host")" "$CLASS" "$host" "$NAME" \
    < "$WORK/ctl" > "$WORK/out" &
  HOLD_PID=$!
  exec 3> "$WORK/ctl" 4< "$WORK/out"
  IFS= read -r line <&4 || true
  case "$line" in
    slot\ *)
      read -r _ SLOT _ NSLOTS _ CORES _ JOBS <<< "$line"
      return 0 ;;
  esac
  exec 3>&- 4<&-
  wait "$HOLD_PID" 2>/dev/null || true
  HOLD_PID=""
  [[ "$line" == busy\ * ]] && { BUSY="${line#busy }"; return 1; }
  return 2
}
# Queued: say so now and every five minutes.
told=-1
queued() {
  local waited=$(($(date +%s) - T0))
  if ((told < 0 || waited >= told + 300)); then
    echo "offload: $1; queued ${waited}s" >&2
    told=$waited
  fi
}

case "$HOST" in
  auto)
    POOL="${STAVE_AUTO_HOSTS:-${STAVE_LINUX_HOST:-} ${STAVE_MACOS_HOST:-}}"
    [[ -n "${POOL// /}" ]] || { echo "offload.sh auto needs STAVE_AUTO_HOSTS or the linux/macos hosts (stave.env.example)" >&2; exit 2; }
    while :; do
      # Hosts with a free slot, least loaded per core first.
      candidates=""
      full=""
      for entry in $POOL; do
        h="${entry%%:*}"
        if [[ "$h" == local ]]; then
          # Local builds fill this disk: leave the pool below
          # STAVE_AUTO_MIN_FREE_GB free (default 30).
          free_gb="$(df -Pk "$ROOT" | awk 'NR==2{printf "%d", $4/1048576}')"
          ((free_gb >= ${STAVE_AUTO_MIN_FREE_GB:-30})) || continue
        fi
        probe="$(runner "$h" probe "$(slot_root "$h")" "$(slots_for "$entry")" "$(long_slot "$h")" "$CLASS" 2> /dev/null)" || continue
        read -r _ free _ _ _ _ _ load <<< "$probe"
        if ((free > 0)); then candidates+="$load $h"$'\n'; else full+=" $h"; fi
      done
      chosen=""
      for h in $(printf '%s' "$candidates" | sort -n | awk '{print $2}'); do
        if take_slot "$h" "$(slots_for "$h")"; then chosen="$h"; break; fi
      done
      [[ -n "$chosen" ]] && break
      queued "no free $CLASS slot on any host (full:${full:- none reachable})"
      sleep 10
    done
    HOST="$chosen"
    ;;
  *)
    want="$(slots_for "$(resolve "$HOST")")"
    HOST="$(resolve "$HOST")"
    HOST="${HOST%%:*}"
    while :; do
      code=0
      take_slot "$HOST" "$want" || code=$?
      ((code == 0)) && break
      ((code == 1)) || { echo "offload: could not reach $HOST" >&2; exit 255; }
      queued "$BUSY"
      sleep 5
    done
    ;;
esac
T_SLOT="$(date +%s)"
echo "offload: $NAME → $HOST slot $SLOT ($CLASS, $JOBS of $CORES cores, queued $((T_SLOT - T0))s)" >&2
event job.started

NICE=10
[[ "$CLASS" == long ]] && NICE=19
printf -v CMD '%q ' "$@"

if [[ "$HOST" == local ]]; then
  # This machine: no sync, this checkout and its own target directory.
  DIR="$ROOT"
  RUN="$CMD"
else
  SYNC="${STAVE_OFFLOAD_SYNC:-rsync}"
  for tar_host in ${STAVE_TAR_HOSTS:-}; do
    [[ "${tar_host%%:*}" == "$HOST" ]] && SYNC="${STAVE_OFFLOAD_SYNC:-tar}"
  done
  SLOTDIR="stave-offload/slots/$SLOT"
  DIR="$SLOTDIR/tree"
  GITENV=""
  if [[ "$SYNC" == tar ]]; then
    # For hosts where rsync hangs (a container that refuses signals between
    # processes: rsync's processes signal each other to finish): send
    # exactly the files git tracks or would track; the host updates the
    # slot's tree from them by content (the runner's `sync`). The tree keeps
    # a git index of exactly those files (no history), so the git-grep tree
    # checks (vocabulary, purity) check the real tree; tag- and
    # history-dependent scripts belong on an rsync host.
    (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard \
      | perl -0 -ne 'chomp; print "$_\0" if -e $_ || -l $_' \
      | COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata -czf - --null -T -) > "$WORK/tree.tgz"
    step runner "$HOST" sync "$SLOTDIR" < "$WORK/tree.tgz"
  else
    # Sources only, compared by content (-c, no -t): a file whose bytes did
    # not change keeps its mtime, so cargo rebuilds only what changed even
    # when another checkout sent the slot's last job. Whatever git ignores
    # (build output, bundles, refs) is not sent, and the slot's target/
    # (also excluded) survives --delete. .git goes whole (rules come
    # first-match-first: gitignore's **/refs/ must not reach .git/refs).
    step rsync -rlpzc --delete \
      --include '/.git/***' \
      --filter=':- .gitignore' \
      --exclude '/target/' --exclude '/.claude/' --exclude '**/refs/' \
      "$ROOT/" "$HOST:$DIR/"
    # A git worktree's .git is a file naming a path on this machine; give the
    # host the repository itself (tags and history, for the docs and release
    # scripts) with this worktree's HEAD and index, and point git at it.
    if [[ -f "$ROOT/.git" ]]; then
      COMMON="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)"
      GITDIR="$(git -C "$ROOT" rev-parse --path-format=absolute --git-dir)"
      step rsync -az --delete --exclude '/worktrees/' --exclude '/HEAD' --exclude '/index' "$COMMON/" "$HOST:$SLOTDIR/git/"
      step rsync -a "$GITDIR/HEAD" "$GITDIR/index" "$HOST:$SLOTDIR/git/"
      GITENV="GIT_DIR=\$HOME/$SLOTDIR/git GIT_WORK_TREE=\$HOME/$DIR "
    fi
  fi
  RUN=". ~/.cargo/env && ${GITENV}CARGO_TERM_COLOR=always $CMD"
fi
T_SYNC="$(date +%s)"

code=0
step runner "$HOST" exec "$(slot_root "$HOST")" "$SLOT" "$DIR" "${STAVE_OFFLOAD_TIMEOUT:-}" "$JOBS" "$NICE" "$HOST" \
  "$RUN" "${STAVE_OFFLOAD_FETCH:-}" <&5 || code=$?
STATUS=$code
T_END="$(date +%s)"

if [[ -n "${STAVE_OFFLOAD_FETCH:-}" && "$HOST" != local ]]; then
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
fi
exit "$STATUS"
