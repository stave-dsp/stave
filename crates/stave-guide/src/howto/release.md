# Cut a release

A framework release is a git tag that plugin suites pin. Nothing is
published to crates.io; `scripts/release.sh` turns a clean `main` into a
tagged, gate-checked version in one command.

## The script

```sh
scripts/release.sh 0.3.0
```

It refuses to start unless the version is `MAJOR.MINOR.PATCH`, the
current branch is `main`, the working tree is clean, and the tag
`v<version>` does not exist yet. Then, in order, it:

1. sets `version` in the workspace `Cargo.toml`'s `[workspace.package]`
   (every crate inherits it) and refreshes `Cargo.lock` with
   `cargo update --workspace`;
2. runs `scripts/semver-checks.sh` against the newest `v*` tag, so the
   new version number must allow the API change since the last release;
3. runs every gate CI runs, whatever changed (`scripts/affected.sh
   --full --no-cache`): the tree rules, clippy, every test, the
   doctests, rustdoc with private items, the MSRV check (its toolchain
   installed first) and the docs site; then `scripts/check-local-leaks.sh`;
4. commits `Release v<version>`, makes an annotated tag `v<version>`,
   and pushes `main` and the tag to every configured remote.

## Semver checks

`scripts/semver-checks.sh [TAG]` runs `cargo-semver-checks` (0.51.0)
over the workspace with the tag as the baseline revision (by default the
newest `v*` tag in `HEAD`'s history). The tools, the example, the test
harness and the guide are not a published API: `stave-bench`,
`stave-play`, `stave-storybook`, `stave-docsite`, `stave-tilt-vst3`, `stave-pluck`,
`stave-test` and `stave-guide` are excluded, and so is every package
the baseline tag does not have yet (the script names each one it
skips). Besides the release script, the `semver` job of the `deep`
workflow runs it on every push to a `release/*` branch, where a patch
release must not break the API it patches.

## After the tag

Plugin suites move to the new framework deliberately. In
[stave-plugins](https://github.com/stave-dsp/stave-plugins):

```sh
scripts/bump-framework.sh v0.3.0
```

The release script prints this line when it finishes.

## Plugins during an API break

The `plugins against this change` job in CI builds and lints
stave-plugins against every framework change. It is not a required
check: between releases `main` may break the plugin API on purpose.
So that the job still says something during a planned break, give it a
stave-plugins branch that follows the break. `scripts/plugins-ref.sh`
picks the first of:

1. a `plugins-branch: <name>` line in the pull request's description
   or the commit message (the branch must exist, or the job fails);
2. a stave-plugins branch with the same name as the pull request's
   branch;
3. a stave-plugins branch `stave-<next>` or `stave-<major.minor>` of
   the release being prepared: the newest `releases/v<next>.md` beyond
   the workspace version (while `releases/v0.5.0.md` waits on 0.4.0,
   `stave-0.5.0` or `stave-0.5`);
4. stave-plugins' `main`.

For a break that lands in several pull requests, keep one
`stave-<next>` branch in stave-plugins migrated as each lands; once
the release is tagged, merge it into stave-plugins' `main` and run
`scripts/bump-framework.sh v<next>` there.
