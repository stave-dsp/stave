#!/usr/bin/env bash
# Fail if any value from this machine's local configuration (host names,
# library and capture paths — see stave.env.example) appears in tracked
# files or in the staged changes. The values themselves never enter the
# repo: the check reads them from ~/.config/stave/env and .stave/local.env.
# With no local configuration there is nothing to check.
#
#   scripts/check-local-leaks.sh            # every tracked file
#   scripts/check-local-leaks.sh --staged   # what is about to be committed
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/scripts/lib/config.sh"
stave_config_load "$ROOT"

values=()
for key in STAVE_LINUX_HOST STAVE_MACOS_HOST STAVE_REFERENCE_HOST STAVE_SAMPLE_LIBRARY STAVE_CAPTURES; do
  [[ -n "${!key:-}" ]] && values+=("${!key}")
done
for host in ${STAVE_TAR_HOSTS:-}; do values+=("$host"); done
((${#values[@]})) || exit 0
unique=()
while IFS= read -r value; do unique+=("$value"); done < <(printf '%s\n' "${values[@]}" | sort -u)
values=("${unique[@]}")

found=0
for value in "${values[@]}"; do
  # Short generic words would match everywhere; only check distinctive values.
  ((${#value} >= 6)) || continue
  if [[ "${1:-}" == --staged ]]; then
    hits="$(git -C "$ROOT" diff --cached -U0 | grep '^+' | grep -vF '+++' | grep -nF -- "$value" || true)"
  else
    hits="$(git -C "$ROOT" grep -nF -- "$value" || true)"
  fi
  if [[ -n "$hits" ]]; then
    echo "local configuration leaked into the repo: \"$value\"" >&2
    printf '%s\n' "$hits" | sed -n '1,5p' >&2
    found=1
  fi
done
if ((found)); then
  echo "Use the config keys (stave.env.example) instead of this machine's values." >&2
  exit 1
fi
