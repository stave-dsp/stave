#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT/scripts/lib/offload-events.sh"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT
export TEST_DIR
STAVE_OFFLOAD_EVENTS='cat >> "$TEST_DIR/events"'
emit '{"kind":"job.started"}'
[[ "$(cat "$TEST_DIR/events")" == '{"kind":"job.started"}' ]]
STAVE_OFFLOAD_EVENTS='cat >/dev/null; echo fixture-error >&2; exit 9'
emit '{"kind":"job.finished"}' 2>"$TEST_DIR/errors"
grep -q fixture-error "$TEST_DIR/errors"
grep -q 'delivery failed' "$TEST_DIR/errors"
# Exercise the in-script heartbeat and cleanup using an isolated local slot.
export XDG_CACHE_HOME="$TEST_DIR/cache" XDG_STATE_HOME="$TEST_DIR/state"
export STAVE_OFFLOAD_EVENTS='cat >> "$TEST_DIR/lifecycle"'
export STAVE_OFFLOAD_HEARTBEAT=1
bash "$ROOT/scripts/offload.sh" local -- sleep 2.2
perl -MJSON::PP -e '
 my @e=map {decode_json($_)} <>;
 die unless $e[0]{kind} eq "job.started" && $e[-1]{kind} eq "job.finished";
 die unless grep {$_->{kind} eq "job.heartbeat" && !exists $_->{detail}} @e;
 for (@e) {die unless $_->{id} eq $e[0]{id}}
' "$TEST_DIR/lifecycle"
count="$(wc -l < "$TEST_DIR/lifecycle")"
sleep 1.2
[[ "$(wc -l < "$TEST_DIR/lifecycle")" == "$count" ]]
export STAVE_OFFLOAD_HEARTBEAT=0
if bash "$ROOT/scripts/offload.sh" local -- sh -c 'exit 7'; then exit 1; else [[ "$?" == 7 ]]; fi
echo 'offload events: all checks passed'
