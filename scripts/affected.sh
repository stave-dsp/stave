#!/usr/bin/env bash
# Run the gates a change can affect, and only those: the change against a
# base ref picks the packages (the ones owning the changed files, plus
# every package depending on them) and whether the docs site is built; a
# tree that already passed the same gates is not gated again. The fast
# inner loop for a working tree, and the selector CI calls.
#
#   scripts/affected.sh                 # the working tree vs where it left origin/main
#   scripts/affected.sh --base REF      # vs REF (CI: the pull request's or merge group's base)
#   scripts/affected.sh --plan          # print the selected gate commands, run nothing
#   scripts/affected.sh --full          # every gate, whatever changed
#   scripts/affected.sh --no-cache      # gate even a tree that already passed
#
# The change is the working tree (uncommitted and untracked files
# included) against the merge base of HEAD and the base ref (default
# origin/main, else main).
#
# The selection:
#
#   whole workspace + site   a root or shared file changed (FULL below:
#                            Cargo.lock, the workspace Cargo.toml,
#                            rust-toolchain.toml, .cargo/, .config/,
#                            deny.toml, scripts/, .github/), or --full
#   whole workspace          the affected packages are 2/3 or more of the
#                            workspace (a kernel crate: stave-core,
#                            stave-dsp, stave-macros, stave-telemetry, ...)
#   affected packages        otherwise (scripts/test-affected.py)
#   + site                   a file the docs site reads changed (SITE below)
#
# The gates, in order: the tree rules (gates.sh's checks, always),
# clippy -D warnings, nextest, doctests, rustdoc with private items and
# the MSRV check over the selected packages, then scripts/docsite.sh.
# Each one's time is printed; a failure stops the run and names the gate.
#
# A tree that passes is recorded in <git common dir>/affected/ (or
# $STAVE_AFFECTED_CACHE), keyed by its git tree hash and the gate set; a
# tree that passed the full set satisfies any set.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# Files that can affect anything: changing one gates everything.
FULL=(Cargo.lock Cargo.toml rust-toolchain.toml .cargo/ .config/ deny.toml scripts/ .github/)
# Files the docs site reads beyond rustdoc (tools/docsite): the guide,
# the site itself, the release notes, the Cost page's data, the pages
# generated from source. A binary's main changes its --help page.
SITE=(crates/stave-guide/ tools/docsite/ releases/ crates/stave-dsp/datasheet.json
  crates/stave-dsp/benches/ crates/stave-dsp/tests/datasheet crates/stave-test/src/plugin_checks.rs
  crates/stave-face/src/theme.rs crates/stave-core/src/params/display.rs)
RULES=(check-file-sizes check-view-bindings check-plugin-rules check-vocabulary check-purity check-golden-sizes)

base_ref="" plan_only=0 full=0 cache=1
while (($#)); do
  case "$1" in
    --base) base_ref="${2:?--base needs a ref}"; shift ;;
    --base=*) base_ref="${1#--base=}" ;;
    --plan) plan_only=1 ;;
    --full) full=1 ;;
    --no-cache) cache=0 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "affected.sh: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done
if [[ -z "$base_ref" ]]; then
  base_ref=origin/main
  git rev-parse -q --verify origin/main >/dev/null || base_ref=main
fi
base="$(git merge-base HEAD "$base_ref")" || { echo "affected.sh: no merge base with $base_ref" >&2; exit 2; }

# The working tree's git tree hash, untracked files included.
worktree_tree() {
  local idx t
  idx="$(mktemp)"
  cp "$(git rev-parse --git-path index)" "$idx"
  t="$(GIT_INDEX_FILE="$idx" git add -A && GIT_INDEX_FILE="$idx" git write-tree)"
  rm -f "$idx"
  echo "$t"
}
tree="$(worktree_tree)"

# ---------------------------------------------------------------------------
# The selection.
changed=()
while IFS= read -r f; do [[ -n "$f" ]] && changed+=("$f"); done < <(
  { git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)
matches() { # FILE PATTERN... — a root file matches itself; a path with a slash, all under it
  local f="$1" p
  shift
  for p in "$@"; do [[ "$f" == "$p" || ( "$p" == */* && "$f" == "$p"* ) ]] && return 0; done
  return 1
}
why="" site=0
((full)) && why="--full"
for f in ${changed[@]+"${changed[@]}"}; do
  if [[ -z "$why" ]] && matches "$f" "${FULL[@]}"; then why="$f changed"; fi
  if matches "$f" "${SITE[@]}" || [[ "$f" == */src/main.rs || "$f" == */src/bin/* ]]; then site=1; fi
done
meta="$(cargo metadata --format-version 1 --no-deps --locked)"
total="$(python3 -c 'import json,sys; print(len(json.load(sys.stdin)["packages"]))' <<<"$meta")"
pkgs=()
if [[ -n "$why" ]]; then
  scope="whole workspace ($why)" site=1
else
  while IFS= read -r p; do [[ -n "$p" ]] && pkgs+=("$p"); done < <(python3 scripts/test-affected.py --list "$base")
  if ((${#pkgs[@]} * 3 >= total * 2)); then
    scope="whole workspace (${#pkgs[@]} of $total packages affected)"
    pkgs=()
  elif ((${#pkgs[@]})); then
    scope="${#pkgs[@]} of $total packages: ${pkgs[*]}"
  else
    scope="none (no package changed)"
  fi
fi

# The gate commands, as NAME<TAB>COMMAND lines.
gates=()
for r in "${RULES[@]}"; do gates+=("rules	bash scripts/$r.sh"); done
if [[ "$scope" != none* ]]; then
  if ((${#pkgs[@]})); then
    sel="$(printf -- '-p %s ' "${pkgs[@]}")" sel="${sel% }" docsel="$sel"
    # Doctests: only packages with a library.
    libsel="$(python3 -c '
import json, sys
want = set(sys.argv[1:])
print(" ".join("-p " + p["name"] for p in json.load(sys.stdin)["packages"]
    if p["name"] in want and any(k in ("lib", "rlib", "proc-macro") for t in p["targets"] for k in t["kind"])))' "${pkgs[@]}" <<<"$meta")"
  else
    sel="--workspace" docsel="" libsel="--workspace"
  fi
  msrv="$(sed -n 's/^rust-version = "\([0-9.]*\)".*/\1/p' Cargo.toml)"
  [[ "$msrv" == *.*.* ]] || msrv="$msrv.0"
  gates+=("clippy	cargo clippy --all-targets --no-deps --locked $sel -- -D warnings")
  gates+=("tests	cargo nextest run --locked --no-tests=pass $sel")
  [[ -n "$libsel" ]] && gates+=("doctests	cargo test --doc --locked --quiet $libsel")
  gates+=("rustdoc	scripts/rustdoc.sh --document-private-items${docsel:+ $docsel}")
  gates+=("msrv	cargo +$msrv check --all-targets --locked $sel")
fi
((site)) && gates+=("site	scripts/docsite.sh target/site")

set_id="$(printf '%s\n' "${gates[@]}" | git hash-object --stdin | cut -c1-12)"
CACHE="${STAVE_AFFECTED_CACHE:-$(git rev-parse --path-format=absolute --git-common-dir)/affected}"
echo "affected: ${#changed[@]} file(s) changed against ${base:0:8} ($base_ref); tree ${tree:0:12}" >&2
echo "affected: cargo gates: $scope; docs site: $( ((site)) && echo yes || echo no)" >&2
if ((plan_only)); then
  printf '%s\n' "${gates[@]}" | cut -f2-
  exit 0
fi
if ((cache)) && [[ -e "$CACHE/$tree.$set_id" || -e "$CACHE/$tree.full" ]]; then
  echo "affected: tree ${tree:0:12} already passed these gates ($(cat "$CACHE/$tree.$set_id" "$CACHE/$tree.full" 2>/dev/null | head -1)); not gating again" >&2
  exit 0
fi

# ---------------------------------------------------------------------------
# The gates.
T0=$SECONDS
times=() last="" t0=$SECONDS
finish() { # NAME — close the timing of the gate group that just ended
  [[ -n "$last" ]] && times+=("$(printf '%-9s %5ss' "$last" $((SECONDS - t0)))")
  last="$1" t0=$SECONDS
}
report() {
  echo "== affected: timings (total $((SECONDS - T0))s)" >&2
  printf '   %s\n' "${times[@]}" >&2
}
for g in "${gates[@]}"; do
  name="${g%%	*}" cmd="${g#*	}"
  [[ "$name" == "$last" ]] || { finish "$name"; echo "== affected: $name" >&2; }
  if [[ "$name" == msrv ]] && ! { command -v rustup >/dev/null && rustup toolchain list | grep -q "^$msrv"; }; then
    times+=("$(printf '%-9s %5s   skipped: no %s toolchain here (CI installs it)' msrv - "$msrv")") last=""
    continue
  fi
  # The tree rules are quiet unless one fails (then it runs again, aloud).
  quiet=/dev/stdout
  [[ "$name" == rules ]] && quiet=/dev/null
  if ! eval "$cmd" >"$quiet"; then
    [[ "$name" == rules ]] && { eval "$cmd" >&2 || true; }
    times+=("$(printf '%-9s %5ss  FAILED' "$name" $((SECONDS - t0)))")
    report
    echo "affected: FAILED at $name: $cmd" >&2
    exit 1
  fi
done
finish ""
report

# Record the pass, unless the tree changed while the gates ran (then what
# passed is unknown).
if [[ "$(worktree_tree)" != "$tree" ]]; then
  echo "affected: the working tree changed while the gates ran; not recording a pass" >&2
  exit 0
fi
mkdir -p "$CACHE"
printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$scope" >"$CACHE/$tree.$set_id"
[[ "$scope" == "whole workspace"* && "$site" == 1 ]] && cp "$CACHE/$tree.$set_id" "$CACHE/$tree.full"
exit 0
