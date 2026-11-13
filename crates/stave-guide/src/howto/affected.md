# Gate what a change affects

`scripts/affected.sh` runs the gates a change can affect, and only those:
the files changed against a base pick the packages to build, lint, test
and document, and whether the docs site is built. A tree that already
passed the same gates is not gated again. It is the fast loop while
working, and the selector CI calls.

```sh
scripts/affected.sh                 # the working tree vs where it left origin/main
scripts/affected.sh --base REF      # vs another base (CI: the pull request's base)
scripts/affected.sh --plan          # print the selected gate commands, run nothing
scripts/affected.sh --outputs       # print the selection as AFFECTED_* lines, run nothing
scripts/affected.sh --full          # every gate, whatever changed
scripts/affected.sh --no-cache      # gate even a tree that already passed
```

The change is the working tree, uncommitted and untracked files included,
against the merge base of `HEAD` and the base (by default `origin/main`).
It needs only git, cargo, cargo-nextest and python3, in a checkout (it
reads the history; to build elsewhere, run the `--plan` commands there).

## The gates

In order, each timed, the first failure stopping the run and naming its
gate:

| gate | command |
|---|---|
| `rules` | the tree rules `scripts/gates.sh` checks: file sizes, view bindings, plugin rules, vocabulary, purity, golden sizes (always) |
| `clippy` | `cargo clippy --all-targets --no-deps --locked … -- -D warnings` |
| `tests` | `cargo nextest run --locked --no-tests=pass …` |
| `doctests` | `cargo test --doc --locked …`, packages with a library |
| `rustdoc` | `scripts/rustdoc.sh --document-private-items …` |
| `msrv` | `cargo +<rust-version> check --all-targets --locked …` (skipped where that toolchain is not installed) |
| `site` | `scripts/docsite.sh` |

`--plan` prints these commands, one per line, to run them elsewhere.

## In CI

CI's jobs each run their own gates, so they ask for the selection
itself: `--outputs` prints it as lines for `$GITHUB_ENV`, and a job's
steps read them.

| line | value |
|---|---|
| `AFFECTED_SCOPE` | `workspace`, `packages` or `none` |
| `AFFECTED_PACKAGES` | cargo's selection: `--workspace`, or `-p a -p b` |
| `AFFECTED_PACKAGE_NAMES` | the selected packages' names (empty for the workspace) |
| `AFFECTED_LIB_PACKAGES` | the doctests' selection (packages with a library) |
| `AFFECTED_DOC_PACKAGES` | `scripts/rustdoc.sh`'s selection (empty: every crate) |
| `AFFECTED_SITE` | `1` if the docs site is built |

A pull request is gated against its base branch, a merge group (main's
merge queue) against the commit the group was built on, so a group of
several pull requests is gated on what they change together; main itself
and a dispatched run are gated in full (`--full`). A job the change
cannot reach skips its gates and still reports success, so every
required check reports on every change. CI never uses the pass cache:
`--outputs` returns before it.

## The selection

What `…` covers, and whether the site is built, depends on the changed
files:

| changed | cargo gates | site |
|---|---|---|
| a root or shared file: `Cargo.lock`, the workspace `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `.config/`, `deny.toml`, `scripts/`, `.github/` | whole workspace | yes |
| files whose packages and dependents are 2/3 or more of the workspace (a kernel crate: `stave-core`, `stave-dsp`, `stave-macros`, `stave-telemetry`, `stave-ui`, …) | whole workspace | if below |
| any other file in a package | the packages owning the changed files, and every package depending on them (`scripts/test-affected.py`) | if below |
| a file the site reads: `crates/stave-guide/`, `tools/docsite/`, `releases/`, the Cost page's data and the sources it shows, a binary's `main.rs` (its `--help` page) | | yes |
| anything else (`README.md`, `DSP.md`, `OVERHAUL.md`, `fuzz/`) | none | no |

Gating only the affected packages holds when the base passed every gate:
a package the change cannot reach is the one that passed there. A package
reading another package's files (`include_str!` across crate
directories) is listed in `READERS` in `scripts/test-affected.py`, so a
change to those files reaches it.

## The pass cache

A tree that passes is recorded in `affected/` in the clone's git
directory (`git rev-parse --git-common-dir`, shared by every worktree; or
`$STAVE_AFFECTED_CACHE`), keyed by the git tree hash and the gate set; a
tree that passed the full set satisfies any set. So the same tree is
never gated twice: running it again after a commit that changes nothing,
or after rebasing onto a base whose change to the tree is nil, returns at
once. A tree that changes while its gates run is not recorded.
