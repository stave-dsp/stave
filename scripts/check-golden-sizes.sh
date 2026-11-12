#!/usr/bin/env bash
# Golden images stay small: every committed `tests/golden/*.png` in the
# tree together under a budget (they are reviewed like code and cloned by
# everyone). Draw goldens at 1× or less, with BoxText.
#
#   scripts/check-golden-sizes.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BUDGET=$((3 * 1024 * 1024))
total=0
while IFS= read -r f; do
  [[ -f "$f" ]] || continue
  total=$((total + $(wc -c <"$f")))
done < <(git ls-files --cached --others --exclude-standard -- '*/tests/golden/*.png')
if ((total > BUDGET)); then
  echo "golden images: $total bytes, over the $BUDGET-byte budget" >&2
  exit 1
fi
