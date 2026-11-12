#!/usr/bin/env bash
# The tree rules as a GATE (the guide: crates/stave-guide/src/howto/project-layout.md). By our own
# doctrine — "laws attach to forms; conventions are deferred
# failures" — style conventions drift unless they fail the build.
# Every check here exists because its violation actually happened.
set -euo pipefail
fail=0

# 1. ONE SPINE per engine: `impl XxxEngine` blocks live in exactly
#    one engine file — components, never distributed extensions.
for proj in examples/*/; do
  src="$proj/src"
  [ -d "$src" ] || continue
  files=$(grep -rl "^impl [A-Za-z]*Engine " "$src" --include="*.rs" 2>/dev/null | grep -v "lib.rs" | sort -u || true)
  n=$(echo "$files" | grep -c . || true)
  if [ "$n" -gt 1 ]; then
    echo "RULE: $proj engine impl blocks are distributed across files (one spine only):"
    echo "$files"
    fail=1
  fi
done

# 2. GROWTH SIGNAL (advisory, never fails): a large single-file
#    engine often means the second concept has arrived. Line count
#    is a proxy, not a property — the decision is human.
for f in examples/*/src/engine.rs; do
  [ -f "$f" ] || continue
  lines=$(wc -l < "$f")
  if [ "$lines" -gt 200 ]; then
    echo "note: $f is $lines lines — worth asking if a second concept wants engine/ components"
  fi
done

# 3. NO ARCHAEOLOGY in any source: milestone tags ((K3), (G6), (F-b4),
#    (Step 5), (ABI 8)) are provenance — commits own that.
TAGS='\((K[0-9]|G[0-9]|F-b[0-9]|Step [0-9]|ABI [0-9]|M-mod)'
if grep -rEn "$TAGS" examples crates tools --include="*.rs" >/dev/null 2>&1; then
  echo "RULE: milestone tags in code comments (provenance lives in commits):"
  grep -rEn "$TAGS" examples crates tools --include="*.rs" | head -5
  fail=1
fi

# 4. DECLARATION-BACKED READS (E1): a cast-and-clamp beside a read
#    restates the declaration and drifts.
if grep -rn 'plain() as f32' examples/*/src --include="*.rs" >/dev/null 2>&1; then
  echo "RULE: 'plain() as f32' in an engine — use p.x.value() (typed, bounded by the declaration):"
  grep -rn 'plain() as f32' examples/*/src --include="*.rs" | head -5
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "plugin rules: ok"
