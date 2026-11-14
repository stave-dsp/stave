#!/usr/bin/env bash
# STAVE_OFFLOAD_EVENTS, if set, is a command that receives one JSON line on
# stdin when a job takes its slot and when it ends (e.g. a dashboard's
# ingest CLI). Wait for delivery or durable spooling; show errors without
# changing the build result.
event() { # kind [outcome: ok|failed] [why]
  [[ -n "${STAVE_OFFLOAD_EVENTS:-}" ]] || return 0
  if ! perl -MJSON::PP -e '
    my ($kind, $label, $host, $class, $slot, $t0, $pid, $outcome, $why) = @ARGV;
    my %e = (kind => $kind, project => "stave", id => "offload-$t0-$pid-$label",
      title => $label, host => $host, slot => $slot, detail => length $why ? "$class slot, $why" : "$class slot", at => time() * 1000);
    $e{outcome} = $outcome if length $outcome;
    print JSON::PP->new->canonical->encode(\%e), "\n";
  ' "$1" "$NAME" "$HOST" "$CLASS" "$SLOT" "$T0" "$$" "${2:-}" "${3:-}" \
    | bash -c "$STAVE_OFFLOAD_EVENTS" >/dev/null; then
    echo "offload: event $1 delivery failed; inspect the ingest error and its durable spool" >&2
  fi
  return 0
}

# Cadence is explicit configuration, shared with the receiving monitor.
HEARTBEAT_PID=""
offload_start_heartbeat() {
  [[ -n "${STAVE_OFFLOAD_EVENTS:-}" && -n "${STAVE_OFFLOAD_HEARTBEAT_SECONDS:-}" ]] || return 0
  if [[ ! "$STAVE_OFFLOAD_HEARTBEAT_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
    echo 'offload: STAVE_OFFLOAD_HEARTBEAT_SECONDS must be positive integer seconds' >&2
    return 2
  fi
  (
    trap - EXIT INT HUP
    sleeper=""
    trap '[[ -z "$sleeper" ]] || kill "$sleeper" 2>/dev/null || true; exit 0' TERM
    while true; do
      sleep "$STAVE_OFFLOAD_HEARTBEAT_SECONDS" & sleeper=$!
      wait "$sleeper" || exit 0
      sleeper=""
      event job.heartbeat
    done
  ) &
  HEARTBEAT_PID=$!
}
offload_stop_heartbeat() {
  [[ -n "$HEARTBEAT_PID" ]] || return 0
  kill "$HEARTBEAT_PID" 2>/dev/null || true
  wait "$HEARTBEAT_PID" 2>/dev/null || true
  HEARTBEAT_PID=""
}
