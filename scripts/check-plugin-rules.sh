#!/usr/bin/env bash
# The tree rules as a GATE (the guide: crates/stave-guide/src/howto/project-layout.md). By our own
# doctrine — "laws attach to forms; conventions are deferred
# failures" — style conventions drift unless they fail the build.
# Every check here exists because its violation actually happened.
set -euo pipefail
fail=0

# 1. ONE SPINE per plugin: the plugin type's `impl Plugin for X` and its
#    inherent `impl X` blocks live in exactly one file — components,
#    never distributed extensions. (The pre-overhaul form, `impl
#    XxxEngine` in engine.rs, no longer exists; matching it tested
#    nothing.)
spines=0
for proj in examples/*/; do
  src="$proj/src"
  [ -d "$src" ] || continue
  for ty in $(grep -rhoE "^impl Plugin for [A-Za-z0-9_]+" "$src" --include="*.rs" | awk '{print $4}' | sort -u); do
    spines=$((spines + 1))
    files=$(grep -rlE "^impl (Plugin for )?${ty}( |\{|<)" "$src" --include="*.rs" | sort -u || true)
    n=$(echo "$files" | grep -c . || true)
    if [ "$n" -gt 1 ]; then
      echo "RULE: $proj $ty impl blocks are distributed across files (one spine only):"
      echo "$files"
      fail=1
    fi
  done
done
# A rule that matches nothing tests nothing: there is always a plugin.
if [ "$spines" -eq 0 ]; then
  echo "RULE: no 'impl Plugin for' under examples/ — the spine check is reading the wrong form"
  fail=1
fi

# 2. GROWTH SIGNAL (advisory, never fails): a large spine file (the one
#    holding `impl Plugin for`) often means a second concept has
#    arrived. Line count is a proxy, not a property — the decision is
#    human.
for f in $(grep -rlE "^impl Plugin for " examples/*/src --include="*.rs" 2>/dev/null); do
  lines=$(wc -l < "$f")
  if [ "$lines" -gt 400 ]; then
    echo "note: $f is $lines lines — worth asking if a second concept wants a module of its own"
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

# 4. DECLARATION-BACKED READS (E1): `value()` is already the param's
#    natural type; a cast-and-clamp beside a read restates the
#    declaration and drifts. (`plain()` is no longer public.)
if grep -rnE '\.value\(\) as f32' examples/*/src --include="*.rs" >/dev/null 2>&1; then
  echo "RULE: 'value() as f32' in a plugin — p.x.value() is typed and bounded by the declaration:"
  grep -rnE '\.value\(\) as f32' examples/*/src --include="*.rs" | head -5
  fail=1
fi

# 5. A DECLARED CPU BUDGET (DSP.md P6): the release perf check holds a
#    plugin only to a budget it states; there is no default to fall
#    back on, so a plugin without one is never checked.
for f in $(grep -rlE "^impl Plugin for " examples/*/src --include="*.rs" 2>/dev/null); do
  if ! grep -qE "^\s*const CPU_BUDGET: Budget = " "$f"; then
    echo "RULE: $f implements Plugin without 'const CPU_BUDGET: Budget = ...' (ns a sample a channel at 48 kHz; see the guide's Measure performance)"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "plugin rules: ok"
