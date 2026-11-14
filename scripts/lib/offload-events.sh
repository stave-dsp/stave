#!/usr/bin/env bash
# Wait for delivery or durable spooling, without retaining slot descriptors.
# The receiver owns retry/spool policy; ingestion errors remain visible.
emit() {
  [[ -n "${STAVE_OFFLOAD_EVENTS:-}" ]] || return 0
  if ! { printf '%s\n' "$1" | bash -c "$STAVE_OFFLOAD_EVENTS" >/dev/null; } 3>&- 4>&- 5>&-; then
    echo "offload: event delivery failed; inspect the ingest error and its durable spool" >&2
  fi
  return 0
}
