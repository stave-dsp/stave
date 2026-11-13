# Per-machine and per-checkout configuration for stave's scripts. Sourced,
# never run. Later sources win:
#
#   1. the defaults below (nothing machine-specific)
#   2. ${XDG_CONFIG_HOME:-~/.config}/stave/env   — this machine
#   3. <repo>/.stave/local.env                   — this checkout (gitignored)
#   4. the environment                           — this command
#
# Each file is plain `KEY=value` shell assignments. stave.env.example at the
# repo root lists every key; the guide's "Local configuration" page explains
# them. Nothing here is required: scripts that need a key say which and how
# to set it.

stave_config_load() {
  local root="$1" key saved
  local keys="STAVE_LINUX_HOST STAVE_MACOS_HOST STAVE_TAR_HOSTS STAVE_REFERENCE_HOST
    STAVE_REFERENCE_NAME STAVE_SAMPLE_LIBRARY STAVE_CAPTURES STAVE_AUTO_HOSTS
    STAVE_AUTO_MAX_LOAD STAVE_AUTO_MIN_FREE_GB"
  # The environment wins: remember what the caller set.
  for key in $keys; do
    if [[ -n "${!key+x}" ]]; then saved+="$key=$(printf '%q' "${!key}") "; fi
  done
  local file
  for file in "${XDG_CONFIG_HOME:-$HOME/.config}/stave/env" "$root/.stave/local.env"; do
    # shellcheck disable=SC1090
    [[ -f "$file" ]] && source "$file"
  done
  [[ -n "${saved:-}" ]] && eval "$saved"
  for key in $keys; do
    # shellcheck disable=SC2163
    [[ -n "${!key+x}" ]] && export "$key"
  done
  return 0
}

# Print a key's value, or fail naming the key and where to set it.
stave_config_require() {
  local key="$1" why="$2"
  if [[ -z "${!key:-}" ]]; then
    echo "$why needs $key: set it in ~/.config/stave/env, .stave/local.env or the environment (see stave.env.example)" >&2
    return 1
  fi
  printf '%s' "${!key}"
}
