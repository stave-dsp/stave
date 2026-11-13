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
| `STAVE_AUTO_HOSTS` | `scripts/offload.sh auto` | the pool `auto` takes a free slot from, least loaded per core first (`local` = this machine; `host:N` = N build slots) |
| `STAVE_AUTO_MIN_FREE_GB` | `scripts/offload.sh auto` | free disk below which `local` leaves the pool (default 30) |
| `STAVE_LINUX_SLOTS`, `STAVE_MACOS_SLOTS`, `STAVE_LOCAL_SLOTS` | `scripts/offload.sh` | build slots on that host: jobs it runs at once (default a quarter of its cores, at least 1; local 1) |
| `STAVE_OFFLOAD_LONG_SLOT` | `scripts/offload.sh` | 1 (default): a build host keeps a separate slot for `long` jobs; 0: they take a build slot |
| `STAVE_OFFLOAD_TIMEOUT` | `scripts/offload.sh` | stop a job after this long (seconds, or `30m`, `2h`); unset, no limit |
| `STAVE_OFFLOAD_EVENTS` | `scripts/offload.sh` | a command fed one JSON line (`job.started` / `job.finished`, with host, slot, outcome) per job event, e.g. a dashboard's ingest; unset, none |
| `STAVE_REFERENCE_HOST` | `scripts/datasheet.sh` | the machine whose numbers are committed to `datasheet.json` |
| `STAVE_REFERENCE_NAME` | `scripts/datasheet.sh` | how the Cost page describes it (the hardware, e.g. "Apple M1, 8 cores") |
| `STAVE_SAMPLE_LIBRARY` | program-material tests, `scripts/corpus.sh`, `stave-play --material role:NAME`, `stave-fit --program` | reference audio (never committed; tests skip when unset); `crates/stave-test/corpus.spec` names the roles it fills |
| `STAVE_CORPUS_LOCK` | `scripts/corpus.sh lock`, the corpus readers | the local lock of each corpus pick's SHA-256 (default `~/.config/stave/corpus.lock`; never committed) |
| `STAVE_CAPTURES` | capture comparisons | hardware captures (never committed; tests skip when unset) |

## Building on another machine

`scripts/offload.sh HOST -- COMMAND` syncs the working tree (including
uncommitted changes, excluding anything git ignores) to an ssh host, runs
the command there under `nice` and returns its exit status, so a laptop
can hand heavy builds to a faster machine:

```sh
scripts/offload.sh linux -- cargo nextest run -p stave-dsp
scripts/offload.sh macos -- scripts/gates.sh
scripts/offload.sh auto -- cargo nextest run   # a free slot on any host, least loaded first
```

The host needs rustup, `cargo-nextest` and perl (every Linux and macOS has
it); the pinned toolchain installs itself. Nothing else is installed: the
host side ships with each call.

**Slots.** Each host runs at most a fixed number of offload jobs at once:
its build slots (default a quarter of its cores, so 2 on a 6- or 8-core
machine; `local` 1; set with `STAVE_LINUX_SLOTS` and friends or `host:N`).
A job takes a free slot or waits for one, saying what holds them; `auto`
takes the least loaded host with a free slot and queues while every host is
full. Naming a host goes through its slots too, so no invocation can
oversubscribe a machine. A job's cargo, nextest, libtest, rayon and OpenMP
threads are held to the slot's share of the cores (cores ÷ slots:
`CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS`, `RUST_TEST_THREADS`,
`RAYON_NUM_THREADS`, `OMP_NUM_THREADS`, and `STAVE_OFFLOAD_JOBS` for
scripts).

Each slot keeps one workspace and its build cache for good. The tree is
synced by content: an unchanged file keeps its timestamp, so a build after
a one-file change recompiles only what that change reaches, whichever
checkout sent the slot's previous job. `STAVE_OFFLOAD_NAME` labels a job
(queue messages, the log); it does not choose a directory.

**Long jobs.** `STAVE_OFFLOAD_CLASS=long` (ngspice batches, fuzzing, kani,
datasheet fits) runs at nice 19, one per host, in the host's long slot, so
it never holds a build slot from the builds queued behind it
(`STAVE_OFFLOAD_LONG_SLOT=0`: it takes a build slot instead).

**Stopping.** The command runs as its own session on the host, with a
pseudo-terminal as its controlling terminal. When the script ends
(finished, interrupted, killed, or its ssh connection dropped) the slot's
holder hangs that terminal up and the kernel delivers SIGHUP to the job's
process group: no job outlives its caller. This works even on a host that
refuses `kill` between processes (a locked-down container, where `timeout`
cannot stop its own child). `STAVE_OFFLOAD_TIMEOUT=2h` stops a job after
that long, printing `offload: TIMEOUT` and exiting 124; unset, a job runs as
long as it needs.

**Log and capacity.** Every job appends a JSON line to
`${XDG_STATE_HOME:-~/.local/state}/stave/offload.jsonl`: label, host, class,
slot, queue wait, sync and run seconds, exit status and whether it timed
out. `scripts/offload.sh stats [HOURS]` summarises the last HOURS (default
24) per host and class: jobs, slot utilisation, p50 and p95 queue wait and
run time, failures and timeouts. Slot
counts, and how many agents the hosts can carry, are read off it.

**Cleaning up.** Before slots, every `STAVE_OFFLOAD_NAME` got its own
workspace and build cache. `scripts/offload.sh gc HOST` removes those,
skipping any whose lock is held, that a process runs in, or that was written
in the last 15 minutes (`--dry-run` lists them).

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
