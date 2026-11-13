#!/usr/bin/env bash
# Land the current branch on main through a local merge queue (the bors
# model, serialised): one lander at a time, so main cannot move while a
# change's gates run, and each change is gated once, against exactly the
# tree that will become main.
#
#   scripts/land.sh            # lock, rebase on origin/main, gate, push HEAD:main
#   scripts/land.sh --check    # the same gates for the working tree, no lock, no push
#   scripts/land.sh --plan     # print what would be gated, run nothing
#   scripts/land.sh --full     # (with any of the above) every gate, whatever changed
#   scripts/land.sh --here     # run the cargo gates on this machine, not offloaded
#
# Landing: take the landing lock (a flock on <git common dir>/land/lock,
# shared by every worktree of this clone), fetch origin, rebase onto
# origin/main, gate, push HEAD:main (a plain push: fast-forward only),
# release the lock. A failure releases the lock, leaves the branch
# rebased, and names the failing step; fix, commit, run it again.
#
# Gates: the tree rules (scripts/gates.sh's checks plus
# check-local-leaks.sh) always run, here; the cargo gates run on a build
# host through `scripts/offload.sh $STAVE_LAND_HOST` (default `auto`) in
# one sync: clippy -D warnings, nextest, doctests, rustdoc with private
# items, the MSRV check, and the docs site. Which packages, and whether the
# site, depends on what changed against origin/main:
#
#   whole workspace + site   a root or shared file changed (FULL below:
#                            Cargo.lock, the workspace Cargo.toml,
#                            rust-toolchain.toml, .cargo/, .config/,
#                            deny.toml, scripts/, .github/), or --full
#   whole workspace          the affected packages are 2/3 or more of
#                            the workspace (a kernel crate: stave-core,
#                            stave-dsp, stave-macros, stave-telemetry, ...)
#   affected packages        otherwise: the packages owning the changed
#                            files and every package depending on them
#                            (scripts/test-affected.py)
#   + site                   a file the docs site reads changed (SITE below)
#
# A tree that passed a gate set is recorded under <git common dir>/land/
# (key: the git tree hash and the gate set); the same tree with the same
# set, or any set when it passed the full one, is not gated again. So a
# `--check` that passes, followed by a landing while main has not moved,
# pushes without gating. Every landing appends a line to land/log.
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

# ---------------------------------------------------------------------------
# The cargo gates, run on the build host:
#   land.sh --gates CARGO SITE [PACKAGE...]
# CARGO is `none`, `workspace` or `packages` (the PACKAGEs); SITE is 0 or 1.
if [[ "${1:-}" == --gates ]]; then
  cargo_scope="$2" site="$3"
  shift 3
  pkgs=("$@") sel=() libsel=()
  if [[ "$cargo_scope" == workspace ]]; then sel=(--workspace) libsel=(--workspace); fi
  for p in ${pkgs[@]+"${pkgs[@]}"}; do sel+=(-p "$p"); done
  # Doctests only for packages with a library.
  if [[ "$cargo_scope" == packages ]]; then
    for p in $(cargo metadata --format-version 1 --no-deps --locked | python3 -c '
import json, sys
want = set(sys.argv[1:])
for p in json.load(sys.stdin)["packages"]:
    if p["name"] in want and any(k in ("lib", "rlib", "proc-macro") for t in p["targets"] for k in t["kind"]):
        print(p["name"])' "${pkgs[@]}"); do libsel+=(-p "$p"); done
  fi
  msrv="$(sed -n 's/^rust-version = "\([0-9.]*\)".*/\1/p' Cargo.toml)"
  [[ "$msrv" == *.*.* ]] || msrv="$msrv.0"
  steps=() failed=""
  step() { # NAME COMMAND...
    local name="$1" t0=$SECONDS
    shift
    echo "== land: $name" >&2
    if "$@"; then steps+=("$(printf '%-9s %5ss' "$name" $((SECONDS - t0)))")
    else steps+=("$(printf '%-9s %5ss  FAILED' "$name" $((SECONDS - t0)))"); failed="$name"; fi
  }
  report() {
    echo "== land: gates on the build host (cargo: $cargo_scope${pkgs[*]:+ ${#pkgs[@]} package(s)}; site: $site)" >&2
    printf '   %s\n' "${steps[@]}" >&2
  }
  gates=()
  [[ "$cargo_scope" != none ]] && gates+=(clippy tests doctests rustdoc msrv)
  [[ "$site" == 1 ]] && gates+=(site)
  for gate in ${gates[@]+"${gates[@]}"}; do
    case "$gate" in
      clippy) step clippy cargo clippy --all-targets --no-deps --locked "${sel[@]}" -- -D warnings ;;
      tests) step tests cargo nextest run --locked --no-tests=pass "${sel[@]}" ;;
      doctests) if ((${#libsel[@]})); then step doctests cargo test --doc --locked --quiet "${libsel[@]}"; fi ;;
      # rustdoc.sh documents the workspace unless given packages.
      rustdoc) step rustdoc scripts/rustdoc.sh --document-private-items ${pkgs[@]+$(printf -- '-p %s ' "${pkgs[@]}")} ;;
      msrv)
        if command -v rustup >/dev/null && rustup toolchain list | grep -q "^$msrv"; then
          step msrv cargo "+$msrv" check --all-targets --locked "${sel[@]}"
        else steps+=("$(printf '%-9s %5s   skipped: no %s toolchain on this host (CI checks it)' msrv - "$msrv")"); fi ;;
      site) step site scripts/docsite.sh target/site ;;
    esac
    if [[ -n "$failed" ]]; then report; echo "== land: FAILED at $failed" >&2; exit 1; fi
  done
  report
  exit 0
fi

# ---------------------------------------------------------------------------
mode=land full=0 host="${STAVE_LAND_HOST:-auto}" ARGS=("$@")
for arg in "$@"; do
  case "$arg" in
    --check) mode=check ;;
    --plan) mode=plan ;;
    --full) full=1 ;;
    --here) host=here ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "land.sh: unknown argument $arg (see --help)" >&2; exit 2 ;;
  esac
done

LAND_DIR="$(git rev-parse --path-format=absolute --git-common-dir)/land"
mkdir -p "$LAND_DIR/pass"
T_START=$SECONDS
timings=()
timed() { # NAME COMMAND... — the lock's descriptor is not inherited
  local name="$1" t0=$SECONDS rc=0
  shift
  "$@" 9>&- || rc=$?
  timings+=("$(printf '%-12s %5ss%s' "$name" $((SECONDS - t0)) "$( ((rc)) && echo '  FAILED')")")
  return $rc
}
summary() {
  ((${#timings[@]})) || return 0
  echo "== land: timings (total $((SECONDS - T_START))s)" >&2
  printf '   %s\n' "${timings[@]}" >&2
}
fail() { # STEP MESSAGE
  summary
  echo "" >&2
  echo "land: FAILED at $1 — $2" >&2
  [[ "$mode" == land && -n "${base:-}" ]] && echo "land: lock released; the branch is left rebased on origin/main (${base:0:8}). Fix, commit, run scripts/land.sh again." >&2
  exit 1
}

# The landing lock: one lander per clone. flock(2) through perl (on every
# Mac and Linux box) on descriptor 9, which this shell keeps open; it is
# released when this script exits, however it exits.
lock() {
  exec 9>>"$LAND_DIR/lock"
  local try='use Fcntl ":flock"; open(my $f, ">&=", 9) or die "fd 9: $!"; exit(flock($f, LOCK_EX | ($ARGV[0] ? LOCK_NB : 0)) ? 0 : 1)'
  if ! perl -e "$try" 1; then
    echo "land: waiting for the landing lock, held by: $(cat "$LAND_DIR/holder" 2>/dev/null || echo '?')" >&2
    perl -e "$try" 0
  fi
  printf '%s (pid %s, %s) since %s\n' "$(git rev-parse --abbrev-ref HEAD)" $$ "${ROOT##*/}" "$(date '+%H:%M:%S')" >"$LAND_DIR/holder"
  trap 'rm -f "$LAND_DIR/holder"' EXIT
}

if [[ "$mode" == land ]]; then
  [[ -z "$(git status --porcelain)" ]] || { echo "land: commit or remove your changes first (git status)" >&2; exit 2; }
  t0=$SECONDS
  lock # not through timed: its descriptor must outlive the call
  timings+=("$(printf '%-12s %5ss' lock $((SECONDS - t0)))")
  timed fetch git fetch --quiet origin main || fail fetch "git fetch origin main failed"
  if ! timed rebase git rebase --quiet origin/main; then
    git rebase --abort 2>/dev/null || true
    fail rebase "the branch conflicts with origin/main; run git rebase origin/main, resolve, and land again"
  fi
  # A commit, not the ref: another worktree's fetch moves the ref.
  base="$(git rev-parse origin/main)"
  [[ "$(git rev-parse HEAD)" != "$base" ]] || { echo "land: nothing to land: HEAD is origin/main" >&2; exit 0; }
else
  git fetch --quiet origin main 2>/dev/null || echo "land: fetch failed; using the last fetched origin/main" >&2
  base="$(git merge-base HEAD origin/main)"
fi
# The working tree as it is, uncommitted and untracked files included
# (what offload.sh sends); when landing, HEAD's tree.
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
# The plan.
t0=$SECONDS
changed=()
while IFS= read -r f; do [[ -n "$f" ]] && changed+=("$f"); done < <(
  { git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)
matches() { # FILE PATTERN...
  local f="$1" p
  shift
  # A root file matches itself; a path with a slash, everything under it.
  for p in "$@"; do [[ "$f" == "$p" || ( "$p" == */* && "$f" == "$p"* ) ]] && return 0; done
  return 1
}
why="" site=0
((full)) && why="--full"
for f in "${changed[@]}"; do
  if [[ -z "$why" ]] && matches "$f" "${FULL[@]}"; then why="$f changed"; fi
  if matches "$f" "${SITE[@]}" || [[ "$f" == */src/main.rs || "$f" == */src/bin/* ]]; then site=1; fi
done
pkgs=()
total="$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["packages"]))')"
if [[ -n "$why" ]]; then
  scope="workspace ($why)" site=1
else
  while IFS= read -r p; do [[ -n "$p" ]] && pkgs+=("$p"); done < <(python3 scripts/test-affected.py --list "$base")
  if ((${#pkgs[@]} * 3 >= total * 2)); then
    scope="workspace (${#pkgs[@]} of $total packages affected)"
    pkgs=()
  elif ((${#pkgs[@]})); then
    scope="${#pkgs[@]} of $total packages: ${pkgs[*]}"
  else
    scope="none (no package changed)"
  fi
fi
cargo_scope=packages
[[ "$scope" == workspace* ]] && cargo_scope=workspace
[[ "$scope" == none* ]] && cargo_scope=none
plan="cargo=$cargo_scope${pkgs[*]:+ ${pkgs[*]}} site=$site"
key="$tree.$(printf '%s' "$plan" | git hash-object --stdin | cut -c1-12)"
timings+=("$(printf '%-12s %5ss' plan $((SECONDS - t0)))")
echo "land: ${#changed[@]} file(s) changed against $(git rev-parse --short "$base")" >&2
echo "land: cargo gates: $scope; docs site: $( ((site)) && echo yes || echo no)" >&2
[[ "$mode" == plan ]] && { printf '   %s\n' "${changed[@]}" >&2; exit 0; }

# ---------------------------------------------------------------------------
# The gates, unless this tree already passed them.
if [[ -e "$LAND_DIR/pass/$key" || -e "$LAND_DIR/pass/$tree.full" ]]; then
  echo "land: tree ${tree:0:12} already passed these gates ($(head -1 "$LAND_DIR/pass/$key" 2>/dev/null || head -1 "$LAND_DIR/pass/$tree.full")); not gating again" >&2
  timings+=("$(printf '%-12s %5s   cached' gates -)")
else
  rules() {
    local s
    for s in check-file-sizes check-view-bindings check-plugin-rules check-vocabulary check-purity check-golden-sizes check-local-leaks; do
      bash "scripts/$s.sh" >/dev/null || { echo "land: scripts/$s.sh failed:" >&2; bash "scripts/$s.sh" >&2 || true; return 1; }
    done
  }
  timed rules rules || fail "tree rules" "a tree rule failed (output above)"
  if [[ "$cargo_scope" != none || "$site" == 1 ]]; then
    gates=(bash scripts/land.sh --gates "$cargo_scope" "$site" ${pkgs[@]+"${pkgs[@]}"})
    if [[ "$host" == here ]]; then
      timed "gates (here)" nice -n 10 "${gates[@]}" || fail "cargo gates" "the failing gate is named above"
    else
      timed "gates (host)" scripts/offload.sh "$host" -- "${gates[@]}" || fail "cargo gates" "the failing gate is named above (offloaded to $host)"
    fi
  fi
  # The gates ran on the tree as it was when they started: if it has
  # changed since, what passed is unknown.
  [[ "$(worktree_tree)" == "$tree" ]] || fail "gates" "the working tree changed while the gates ran; leave it alone until they finish, and run again"
  printf '%s %s\n%s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$mode" "$plan" >"$LAND_DIR/pass/$key"
  [[ "$plan" == "cargo=workspace site=1" ]] && cp "$LAND_DIR/pass/$key" "$LAND_DIR/pass/$tree.full"
fi

if [[ "$mode" == land ]]; then
  if ! timed push git push --quiet origin HEAD:main; then
    # main moved outside the queue (a push without scripts/land.sh):
    # land again, from the fetch, a few times.
    attempt="${STAVE_LAND_ATTEMPT:-1}"
    if git fetch --quiet origin main 9>&- && ! git merge-base --is-ancestor origin/main HEAD && ((attempt < 3)); then
      summary
      echo "land: main moved outside the queue while the gates ran; landing again (attempt $((attempt + 1)) of 3)" >&2
      STAVE_LAND_ATTEMPT=$((attempt + 1)) exec "$ROOT/scripts/land.sh" ${ARGS[@]+"${ARGS[@]}"}
    fi
    fail push "the push was refused (main moved outside the queue again, or no push access); land again"
  fi
  echo "land: landed $(git rev-parse --short HEAD) on main" >&2
fi
summary
printf '%s %s %s %ss %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$mode" "$(git rev-parse --short HEAD)" $((SECONDS - T_START)) "$plan" >>"$LAND_DIR/log"
