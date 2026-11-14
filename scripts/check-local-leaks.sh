#!/usr/bin/env bash
# Fail if any value from this machine's local configuration (host names,
# library and capture paths, signing identities and their team IDs, the
# notary profile — see stave.env.example) appears in tracked
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
for key in STAVE_LINUX_HOST STAVE_MACOS_HOST STAVE_REFERENCE_HOST STAVE_SAMPLE_LIBRARY STAVE_CORPUS_LOCK STAVE_CAPTURES \
  STAVE_SIGN_IDENTITY STAVE_NOTARY_PROFILE STAVE_INSTALLER_IDENTITY; do
  [[ -n "${!key:-}" ]] && values+=("${!key}")
done
# A signing identity's team ID leaks on its own too ("… (ABCDE12345)").
for key in STAVE_SIGN_IDENTITY STAVE_INSTALLER_IDENTITY; do
  if [[ "${!key:-}" =~ \(([A-Z0-9]{10})\)$ ]]; then
    values+=("${BASH_REMATCH[1]}")
  fi
done
for host in ${STAVE_TAR_HOSTS:-} ${STAVE_AUTO_HOSTS:-}; do
  host="${host%%:*}"
  [[ "$host" == local ]] || values+=("$host")
done

# Whatever this machine's configuration: no real signing identity is ever
# written into the tree (the examples use <Your Name> (<TEAM_ID>)).
identity_re='Developer ID (Application|Installer): [^<"$]+\([A-Z0-9]{10}\)'
if [[ "${1:-}" == --staged ]]; then
  identities="$(git -C "$ROOT" diff --cached -U0 | grep '^+' | grep -vF '+++' | grep -nE -- "$identity_re" || true)"
else
  identities="$(git -C "$ROOT" grep -nE -- "$identity_re" || true)"
fi
if [[ -n "$identities" ]]; then
  echo "a signing identity is written into the repo (set STAVE_SIGN_IDENTITY in your local configuration instead):" >&2
  printf '%s\n' "$identities" | sed -n '1,5p' >&2
  exit 1
fi
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
