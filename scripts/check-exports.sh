#!/usr/bin/env bash
# Every plugin binary exports the entry points its platform's hosts look
# up — loaded the way a host loads it (dlopen / LoadLibrary, via Python's
# ctypes) and each symbol resolved:
#
#   all       GetPluginFactory, clap_entry
#   macOS     bundleEntry, bundleExit, StaveAUFactory (the Audio Unit)
#   Linux     ModuleEntry, ModuleExit
#   Windows   InitDll, ExitDll
#
# Usage: scripts/check-exports.sh [release|debug]   (builds nothing)
set -euo pipefail
source "$(dirname "$0")/lib/bundle-common.sh"
PROFILE="${1:-release}"
PY=$(command -v python3 || command -v python) || { echo "python not found" >&2; exit 2; }

case "$STAVE_OS" in
  macos) symbols="GetPluginFactory clap_entry bundleEntry bundleExit StaveAUFactory" ;;
  linux) symbols="GetPluginFactory clap_entry ModuleEntry ModuleExit" ;;
  windows) symbols="GetPluginFactory clap_entry InitDll ExitDll" ;;
esac

fail=0
while IFS= read -r crate; do
  lib=$(lib_path "$crate" "$PROFILE")
  [[ "$STAVE_OS" == windows ]] && lib=$(cygpath -w "$lib")
  if "$PY" - "$lib" $symbols <<'PY'
import ctypes, sys
lib = ctypes.CDLL(sys.argv[1])
missing = [s for s in sys.argv[2:] if not hasattr(lib, s)]
print(f"{sys.argv[1]}: " + (f"MISSING {' '.join(missing)}" if missing else f"exports {' '.join(sys.argv[2:])}"))
sys.exit(1 if missing else 0)
PY
  then :; else fail=1; fi
done < <(plugin_crates)
exit "$fail"
