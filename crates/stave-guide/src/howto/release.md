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
3. runs `scripts/gates.sh`: the tree rules, every test and the
   doctests;
4. commits `Release v<version>`, makes an annotated tag `v<version>`,
   and pushes `main` and the tag to every configured remote.

## Semver checks

`scripts/semver-checks.sh [TAG]` runs `cargo-semver-checks` (0.51.0)
over the workspace with the tag as the baseline revision (by default the
newest `v*` tag in `HEAD`'s history). The tools, the example, the test
harness and the guide are not a published API: `stave-bench`,
`stave-play`, `stave-storybook`, `stave-docsite`, `stave-tilt-vst3`,
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
