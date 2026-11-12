#!/usr/bin/env bash
# File-size SIGNALS (the guide: crates/stave-guide/src/howto/project-layout.md) — never failures.
# Size is a proxy for mixed concerns; the split decision is human
# (ratified 2026-07-08: the hard caps + shrink-only ratchet became
# signals — the ratchet had forced several genuinely good cohesion
# moves, but a gate that argues about line counts argues about the
# proxy, not the problem). Thresholds: src 1000 lines, tests 1600.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
signals=0
while IFS= read -r f; do
  n=$(wc -l < "$f" | tr -d ' ')
  rel=${f#"$ROOT/"}
  case "$rel" in
    */tests/*|*/tests.rs) cap=1600 ;;
    *)         cap=1000 ;;
  esac
  if (( n > cap )); then
    echo "size signal: $rel = $n lines (threshold $cap) — worth asking what wants to split"
    signals=$((signals + 1))
  fi
done < <(find "$ROOT/crates" "$ROOT/tools" "$ROOT/examples" -name '*.rs' -not -path '*/target/*')
if (( signals == 0 )); then
  echo "file sizes: no signals"
fi
exit 0
