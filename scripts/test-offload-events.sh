#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT/scripts/lib/offload-events.sh"
TEST_DIR="$(mktemp -d)"
trap 'offload_stop_heartbeat; rm -rf "$TEST_DIR"' EXIT
NAME=test HOST=fixture CLASS=normal SLOT=1 T0=100
export TEST_DIR
STAVE_OFFLOAD_EVENTS='cat >> "$TEST_DIR/events"'
event job.started
event job.finished ok
perl -MJSON::PP -e 'my @e=map {decode_json($_)} <>; die unless @e==2 && $e[0]{kind} eq "job.started" && $e[1]{outcome} eq "ok" && $e[0]{id} eq $e[1]{id}' "$TEST_DIR/events"
STAVE_OFFLOAD_EVENTS='cat >/dev/null; echo fixture-error >&2; exit 9'
event job.started 2>"$TEST_DIR/errors"
grep -q fixture-error "$TEST_DIR/errors"
grep -q 'delivery failed' "$TEST_DIR/errors"
STAVE_OFFLOAD_EVENTS='cat >> "$TEST_DIR/events"'
STAVE_OFFLOAD_HEARTBEAT_SECONDS=1
offload_start_heartbeat
sleep 1.2
offload_stop_heartbeat
grep -q job.heartbeat "$TEST_DIR/events"
count="$(wc -l < "$TEST_DIR/events")"
sleep 1.2
[[ "$(wc -l < "$TEST_DIR/events")" == "$count" ]]
STAVE_OFFLOAD_HEARTBEAT_SECONDS=bad
if offload_start_heartbeat 2>/dev/null; then exit 1; fi
echo 'offload events: all checks passed'
