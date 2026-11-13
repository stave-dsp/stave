# Local configuration

Stave's scripts never hard-code a machine, a host or a path on someone's
disk. What differs between machines lives in local configuration, which is
never committed:

| Where | Scope |
|---|---|
| `~/.config/stave/env` (or `$XDG_CONFIG_HOME/stave/env`) | this machine, every checkout |
| `.stave/local.env` in the repository (gitignored) | this checkout |
| the environment | this command |

Later sources win. Each file is plain `KEY=value` shell assignments, loaded by
`scripts/lib/config.sh`. `stave.env.example` at the repository root lists
every key; copy what you need. Every key is optional: a script that needs
one says which, and how to set it.

| Key | Used by | Meaning |
|---|---|---|
| `STAVE_LINUX_HOST` | `scripts/offload.sh linux` | an ssh destination to build and test on (Linux covers the whole workspace) |
| `STAVE_MACOS_HOST` | `scripts/offload.sh macos` | an ssh destination for the Cocoa crates and macOS-only checks |
| `STAVE_TAR_HOSTS` | `scripts/offload.sh` | hosts that get a tarball instead of rsync (space-separated) |
| `STAVE_REFERENCE_HOST` | `scripts/datasheet.sh` | the machine whose numbers are committed to `datasheet.json` |
| `STAVE_REFERENCE_NAME` | `scripts/datasheet.sh` | how the Cost page describes it (the hardware, e.g. "Apple M1, 8 cores") |
| `STAVE_SAMPLE_LIBRARY` | program-material tests | reference audio (never committed; tests skip when unset) |
| `STAVE_CAPTURES` | capture comparisons | hardware captures (never committed; tests skip when unset) |

## Building on another machine

`scripts/offload.sh HOST -- COMMAND` syncs the working tree (including
uncommitted changes, excluding anything git ignores) to an ssh host, runs
the command there under `nice` and returns its exit status, so a laptop
can hand heavy builds to a faster machine:

```sh
scripts/offload.sh linux -- cargo nextest run -p stave-dsp
scripts/offload.sh macos -- scripts/gates.sh
```

The host needs rustup and `cargo-nextest`; the pinned toolchain installs
itself. Each checkout gets its own directory and build cache there.

## Keeping local values out of the repo

`scripts/check-local-leaks.sh` fails when any value from your local
configuration — a host name, a library path — appears in tracked files
(`--staged`: in what is about to be committed). Run it from a pre-commit
hook so a slip never lands:

```sh
printf '#!/bin/sh\nexec scripts/check-local-leaks.sh --staged\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

Pick host aliases that are distinctive (not a crate or project name), so
the check has nothing to confuse them with.
