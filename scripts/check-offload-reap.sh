#!/usr/bin/env bash
# The offload reaper (scripts/offload.sh reap; the runner's `reap` mode)
# against a staged host in a scratch home: it must find and stop exactly
# the orphaned offload jobs — a slot job whose holder is gone, a process
# that left that job's group, pre-slots jobs whose session is gone — and
# leave alone a job whose slot is held and any process not tied to the
# offload directory; --dry-run stops nothing; --pgid reaps exactly the named
# group; each reap is recorded once and forwarded to the client once; the
# client side logs and reports each reap.
#
#   scripts/check-offload-reap.sh
#
# The staged directory has its own name, so the reaper under test cannot
# match a real offload job of the user running this.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$ROOT/scripts/lib"
T="$(mktemp -d "${TMPDIR:-/tmp}/reap-check.XXXXXX")"
T="$(cd "$T" && pwd -P)"
PIDS=()
cleanup() {
  for p in "${PIDS[@]}"; do kill -KILL -- "-$p" "$p" 2> /dev/null || true; done
  rm -rf "$T"
}
trap cleanup EXIT

fail() { echo "check-offload-reap: FAIL: $*" >&2; exit 1; }
DIR=offload-check
OFF="$T/home/$DIR"
# Behind a symlink, as a host may keep it (working directories then show
# the real path; command lines, the link's).
mkdir -p "$T/store/slots/1/tree" "$T/store/slots/2/tree" "$T/store/oldjob" "$T/home" "$T/elsewhere"
ln -s "$T/store" "$OFF"

# A sleeper that is a session (and group) leader, started from CWD with TAG
# in its arguments and reparented away from us, as an orphan is; prints its
# pid. With SPLIT, it also forks a child into a group of its own in the same
# session, and prints both pids.
sleeper() { # CWD TAG [SPLIT]
  local f="$T/pid.$RANDOM$RANDOM"
  ( perl -MPOSIX=setsid,setpgid -e '
      my ($cwd, $f, $tag, $split) = @ARGV;
      setsid();
      chdir $cwd or die;
      my @pids = ($$);
      if ($split) {
        pipe my $r, my $w;
        my $kid = fork // die;
        if (!$kid) { setpgid(0, 0); close $w; close $r; exec "sh", "-c", "sleep 300; : $tag" }
        push @pids, $kid;
      }
      open my $o, ">", "$f.tmp" or die; print $o "@pids"; close $o; rename "$f.tmp", $f;
      exec "sh", "-c", "sleep 300; : $tag"' "$1" "$f" "$2" "${3:-}" > /dev/null 2>&1 & )
  for _ in $(seq 50); do
    [[ -s "$f" ]] && break
    sleep 0.1
  done
  [[ -s "$f" ]] || fail "a sleeper did not start"
  cat "$f"
  rm -f "$f"
}
alive() { kill -0 -- "-$1" 2> /dev/null || [[ -n "$(ps -o pid= -p "$1" 2> /dev/null)" ]]; }

read -r SLOT_ORPHAN STRAY <<< "$(sleeper "$OFF/slots/1/tree" "" split)"
PIDS+=("$SLOT_ORPHAN" "$STRAY")
HELD="$(sleeper "$OFF/slots/2/tree" "")"; PIDS+=("$HELD")
# Pre-slots jobs, known by their arguments: relative to the home directory
# (where that script started them), or absolute.
LEGACY="$(sleeper "$T/home" "$DIR/argjob.lock")"; PIDS+=("$LEGACY")
LEGACY_ABS="$(sleeper "$T/elsewhere" "$OFF/absjob.target")"; PIDS+=("$LEGACY_ABS")
OTHER="$(sleeper "$T/elsewhere" "")"; PIDS+=("$OTHER")
# Where another process's working directory is readable (some containers
# refuse it), a pre-slots job is also known by it, and a relative path
# from anywhere but home is no such job.
CWD_VISIBLE=0
if [[ -d /proc/self ]]; then readlink "/proc/$OTHER/cwd" > /dev/null 2>&1 && CWD_VISIBLE=1
else CWD_VISIBLE=1; fi
LEGACY_CWD="" OTHER_REL=""
if ((CWD_VISIBLE)); then
  LEGACY_CWD="$(sleeper "$OFF/oldjob" "")"; PIDS+=("$LEGACY_CWD")
  OTHER_REL="$(sleeper "$T/elsewhere" "$DIR/argjob.lock")"; PIDS+=("$OTHER_REL")
fi
ORPHANS=("$SLOT_ORPHAN" "$STRAY" "$LEGACY" "$LEGACY_ABS" ${LEGACY_CWD:+"$LEGACY_CWD"})
OUTSIDE=("$OTHER" ${OTHER_REL:+"$OTHER_REL"})

# Slot 1 recorded a job and its holder is gone (lock free); slot 2's
# holder is alive and holds the lock.
printf '%s' "$SLOT_ORPHAN" > "$OFF/slots/1.pgid"
printf 'lab-one (since then)' > "$OFF/slots/1.owner"
printf 'offload-1-2-lab-one' > "$OFF/slots/1.job"
: > "$OFF/slots/1.lock"
printf '%s' "$HELD" > "$OFF/slots/2.pgid"
printf 'lab-two (since then)' > "$OFF/slots/2.owner"
perl -MFcntl=:flock -e 'open my $f, ">>", $ARGV[0] or die; flock $f, LOCK_EX or die; print "held\n"; sleep 300' \
  "$OFF/slots/2.lock" > "$T/held" &
HOLDER=$!
disown "$HOLDER"
PIDS+=("$HOLDER")
for _ in $(seq 50); do [[ -s "$T/held" ]] && break; sleep 0.1; done

RUNNER="$(base64 < "$LIB/offload-runner.pl" | tr -d '\n')"
reap() { # DRY LEGACY PGIDS FORWARD
  local a args=()
  for a in "$DIR" "$1" "$2" "$3" 2 testhost "$4"; do args+=("$(printf 'x%s' "$a" | base64 | tr -d '\n')"); done
  HOME="$T/home" perl -MMIME::Base64 -e 'eval decode_base64(shift); die $@ if $@' "$RUNNER" reap "${args[@]}"
}
action_of() { # OUTPUT PGID → the action reported for that group
  printf '%s\n' "$1" | perl -MJSON::PP -ne 'print decode_json($1)->{action} if /^found (\{.*\})$/ && decode_json($1)->{pgid} == '"$2"
}

out="$(reap 1 "" "" "")"
for p in "${ORPHANS[@]}"; do
  [[ "$(action_of "$out" "$p")" == "would reap" ]] || fail "dry run: $p not listed: $out"
done
[[ "$(action_of "$out" "$HELD")" == "kept" ]] || fail "dry run: the held slot's job not kept: $out"
for p in "${OUTSIDE[@]}"; do
  [[ -z "$(action_of "$out" "$p")" ]] || fail "dry run: listed $p, which is not an offload job: $out"
done
for p in "${ORPHANS[@]}" "$HELD" "${OUTSIDE[@]}"; do alive "$p" || fail "the dry run stopped $p"; done

out="$(reap "" "" "" 1)"
for p in "${ORPHANS[@]}"; do
  [[ "$(action_of "$out" "$p")" == reaped ]] || fail "not reaped: $p: $out"
  ! alive "$p" || fail "reported reaped but alive: $p"
done
alive "$HELD" || fail "reaped the job of a held slot"
for p in "${OUTSIDE[@]}"; do alive "$p" || fail "reaped $p, which is not an offload job"; done
[[ ! -e "$OFF/slots/1.pgid" ]] || fail "slot 1's job record left behind"
[[ "$(wc -l < "$OFF/reaped.jsonl")" -eq "${#ORPHANS[@]}" ]] || fail "reaped.jsonl: want ${#ORPHANS[@]} lines"
grep -q '"job":"offload-1-2-lab-one"' "$OFF/reaped.jsonl" || fail "the slot job's id was not recorded"

# Nothing left to forward: this run's reaps were shown to this client.
out="$(reap "" "" "" 1)"
! grep -q '^logged ' <<< "$out" || fail "forwarded a reap twice"

# --pgid: exactly that group, even with its slot held; never one that is
# not an offload job.
out="$(reap "" "" "$HELD" 1)"
[[ "$(action_of "$out" "$HELD")" == reaped ]] || fail "--pgid did not reap $HELD: $out"
! alive "$HELD" || fail "--pgid: $HELD alive"
out="$(reap "" "" "$OTHER" 1)"
grep -q "^note pgid $OTHER is not an offload job" <<< "$out" || fail "--pgid on a process outside the offload directory not refused"
for p in "${OUTSIDE[@]}"; do alive "$p" || fail "--pgid reaped $p"; done

# The client side: a table, a job-log line and a job.finished per reap.
printf '%s\n' \
  'found {"action":"reaped","age_s":42,"at":1700000000,"command":"sleep 300","cpu":1.5,"host":"h","job":"offload-1-2-lab","kind":"slot","label":"lab","pgid":7,"procs":1,"slot":"1","why":"gone"}' \
  'logged {"action":"reaped","age_s":5,"at":1700000001,"command":"x","cpu":0,"host":"h","kind":"legacy","label":"old","pgid":8,"procs":1,"why":"gone"}' \
  'summary 1 offload job groups: 1 reaped, 0 kept' \
  | perl "$LIB/offload-reap.pl" "$T/offload.jsonl" > "$T/report"
grep -q '^reaped  *pgid 7 ' "$T/report" || fail "client: no table row"
[[ "$(grep -c '^event ' "$T/report")" -eq 2 ]] || fail "client: want 2 events"
grep -q '"id":"offload-1-2-lab"' "$T/report" || fail "client: the slot job's event id was lost"
grep -q '"id":"offload-reaped-h-8-old"' "$T/report" || fail "client: the pre-slots job's event id"
grep -q '"kind":"job.finished"' "$T/report" || fail "client: event kind"
[[ "$(wc -l < "$T/offload.jsonl")" -eq 2 ]] || fail "client: want 2 job-log lines"
perl "$LIB/offload-stats.pl" 24 "$T/offload.jsonl" > /dev/null || fail "stats chokes on reap lines"

echo "check-offload-reap: ok ($(uname -s); working directories $( ((CWD_VISIBLE)) && echo readable || echo unreadable))"
