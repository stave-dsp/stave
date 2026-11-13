#!/usr/bin/env bash
# The reference corpus (DSP.md R22) against this machine's sample library:
# which roles crates/stave-test/corpus.spec can fill from the library
# STAVE_SAMPLE_LIBRARY names, and whether the files still match the
# checksums the local lock recorded. Reads the library; never copies,
# writes or commits any of it.
#
#   scripts/corpus.sh list [ROLE...]   each role's picks, seconds and rates
#   scripts/corpus.sh check            missing roles, gaps and their fill, lock
#                                      drift; exit 1 on a problem
#   scripts/corpus.sh lock             record each pick's SHA-256, size and
#                                      time in STAVE_CORPUS_LOCK (default
#                                      ~/.config/stave/corpus.lock)
#
# On a build host with a synced copy of the library, set
# STAVE_SAMPLE_LIBRARY there and run `check`: a copy that drifted from the
# lock taken here says so (copy this machine's lock beside it first).
# Without a library it says "skipped: library absent" and exits 0.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/lib/config.sh"
stave_config_load "$ROOT"
exec python3 "$ROOT/scripts/corpus.py" "$@"
