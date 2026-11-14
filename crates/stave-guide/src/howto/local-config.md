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
| `STAVE_OFFLOAD_EVENTS` | `scripts/offload.sh` | a command fed one JSON line (`job.started` / `job.heartbeat` / `job.finished`, with host, slot, outcome) per job event, e.g. a dashboard's ingest; unset, none |
| `STAVE_OFFLOAD_HEARTBEAT` | `scripts/offload.sh` | seconds between `job.heartbeat` events while a job holds its slot (default 60; 0: none) |
| `STAVE_REFERENCE_HOST` | `scripts/datasheet.sh` | the machine whose numbers are committed to `datasheet.json` |
| `STAVE_REFERENCE_NAME` | `scripts/datasheet.sh` | how the Cost page describes it (the hardware, e.g. "Apple M1, 8 cores") |
| `STAVE_SAMPLE_LIBRARY` | program-material tests, `scripts/corpus.sh`, `stave-play --material role:NAME`, `stave-fit --program` | reference audio (never committed; tests skip when unset); `crates/stave-test/corpus.spec` names the roles it fills |
| `STAVE_CORPUS_LOCK` | `scripts/corpus.sh lock`, the corpus readers | the local lock of each corpus pick's SHA-256 (default `~/.config/stave/corpus.lock`; never committed) |
| `STAVE_CAPTURES` | capture comparisons | hardware captures (never committed; tests skip when unset) |
| `STAVE_SIGN_IDENTITY` | `scripts/package.sh sign` | the macOS Developer ID Application identity, by name or SHA-1 ([Ship your plugin](crate::howto::ship)); unset, bundles stay ad-hoc signed |
| `STAVE_NOTARY_PROFILE` | `scripts/package.sh sign` | the `notarytool` keychain profile; unset or missing, no notarisation (the command to make it is printed) |
| `STAVE_INSTALLER_IDENTITY` | `scripts/package.sh sign` | a Developer ID Installer identity: a `.pkg` beside the `.dmg` |

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

**A Windows PC.** A Windows machine serves as a Linux build host through
WSL: install Ubuntu in WSL with a Linux host's tools (rustup,
`cargo-nextest`, a C toolchain, the X11 and ALSA development libraries,
`rsync`), and have Windows' OpenSSH server start that shell: its
`DefaultShell` (an administrator's registry setting) must be
`C:\Windows\System32\bash.exe`, which accepts the `-c COMMAND` the server
passes and lands in the default distribution (`wsl.exe` itself refuses
`-c`; PowerShell would need every command wrapped). Commands then arrive as
on any Linux host, except that the shell starts in the Windows profile
(`/mnt/c/Users/...`); `offload.sh` begins every remote command with `cd`,
so the slots live in the distribution's own home directory on its own
disk. Size it with `host:N` like any other host; `.wslconfig` in the
Windows profile caps the distribution's cores and memory, and `nproc`
inside it is what the slots divide.

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

## Lifecycle and cleanup

Work that is done, lost or failed should not linger: not a job on a build
host, not a worktree, branch or waiting shell on the machine that
coordinates the agents. Each kind has one tool, and each tool lists before
it acts.

| What lingers | What finds it | When | By hand |
|---|---|---|---|
| a job still running after its caller is gone | the slot holder (hangs up the job's terminal) | at once | — |
| a job that slipped past that (holder killed, a process that left the job's group, a pre-slots job) | `offload.sh reap` on the build host | every 10 minutes, once scheduled | `scripts/offload.sh reap HOST --dry-run` |
| a job the event stream still shows as running | the stream's consumer, from missing heartbeats | its own schedule | — |
| a finished agent's worktree | `scripts/janitor.sh worktrees` | when the coordinator runs it | `--apply` |
| a merged agent branch, local or remote | `scripts/janitor.sh branches` | when the coordinator runs it | prints the remote command |
| a shell waiting in a polling loop for something that will not come | `scripts/janitor.sh shells` | when the coordinator runs it | prints the `kill` |
| a pre-slots workspace | `scripts/offload.sh gc` | by hand | `--dry-run` |

**Heartbeats.** With `STAVE_OFFLOAD_EVENTS` set, a job that holds a slot
sends `{"kind":"job.heartbeat","id":…,"at":…}` every
`STAVE_OFFLOAD_HEARTBEAT` seconds (default 60) from this machine, in the
background, until it gives the slot back. A consumer that stops hearing them
can mark the job lost instead of showing it running for ever.

**Reaping a build host.** `scripts/offload.sh reap HOST` lists the offload
jobs on HOST that nobody owns any more, with each one's age, CPU, command and
reason, and stops each one's whole process group: TERM, then KILL after
`--grace` seconds (default 10). It considers only the ssh user's own
processes tied to its offload directory, so nothing else on the host is
reachable. A job counts as orphaned when

- its slot's lock is free: the slot holder lives exactly as long as the
  caller's ssh session, so the job has no caller;
- it left the group of an idle slot's job (the job's stop signals the group
  and missed it) or sits in an idle slot's workspace;
- it is a job of the pre-slots script (`stave-offload/<name>`,
  `stave-offload/<name>.lock`) whose ssh session is gone. One whose session
  is still open (a stale connection can hold it) is listed, and reaped only
  with `--legacy` or `--pgid N`.

```sh
scripts/offload.sh reap linux --dry-run     # list; stop nothing
scripts/offload.sh reap linux               # stop the orphans
scripts/offload.sh reap linux --pgid 4242   # stop exactly this job group
```

Each reap is appended to the host's `stave-offload/reaped.jsonl`, to this
machine's job log (a line with `"kind":"reap"`, which `stats` skips) and to
the event stream as a `job.finished` with outcome `failed` and a
`reaped (lost): …` detail, under the job's own id where the slot recorded
it.

**Scheduling the reaper.** `scripts/offload.sh reap HOST --install [--every
MIN]` installs it inside the host account itself (never system-wide): the
runner goes to `~/stave-offload/bin/`, and a systemd user timer
(`stave-offload-reap.timer`) runs it on Linux; on macOS a launchd agent,
or the account's crontab where launchd refuses an agent to an account
with no login session (a hidden build account reached only by ssh).
`--uninstall` removes it. A scheduled run reaps only what is plainly
orphaned (never `--legacy`); its reaps reach this machine's log and events
on the next `reap HOST`. On Linux, a user timer runs between sessions only
with lingering on, which takes the machine's (or container's)
administrator: `loginctl enable-linger <user>`.

**Agent worktrees and branches.** `scripts/janitor.sh` (on the machine
whose checkout holds `.claude/worktrees`) lists what finished agents left:

```sh
scripts/janitor.sh worktrees                   # list; remove nothing
scripts/janitor.sh worktrees --apply           # remove the removable ones
scripts/janitor.sh branches                    # remote and local agent branches
scripts/janitor.sh shells --hours 6            # stale polling shells
```

A worktree is removable only when nothing would be lost and nothing uses
it: no uncommitted changes, no process working in it, and its branch merged
(every commit on `origin/main`, or its pull request merged at exactly the
branch's tip, as squash merges leave it) or its pull request closed with the
tip pushed (the branch is kept). A branch with no commits of its own and no
pull request may belong to an agent still starting, so it counts only with
`--idle HOURS`; a worktree locked by a live session needs
`--include-locked`. `--apply` runs `git worktree remove` (its `target/` goes
with it) and deletes the branch when merged. Remote agent branches are never
deleted here: the report prints the `git push origin --delete` command for
the merged ones and lists closed-unmerged ones apart; merged local branches
outside any worktree are deleted with `--apply`.

`shells` lists shells that Claude Code started (their command line sources
a `.claude/shell-snapshots` file) running longer than `--hours` (default 6;
0: all) that only wait in a polling loop, with what each waits on where
that can be checked (a pid that is gone, a file not written for a day, or a
`pgrep -f` pattern that only the watcher's own command line matches, so it
never ends), and stopped ssh multiplexers, which can hold sessions open on a
build host. It prints the `kill` command and kills nothing.

## Keeping local values out of the repo

`scripts/check-local-leaks.sh` fails when any value from your local
configuration — a host name, a library path — appears in tracked files
(`--staged`: in what is about to be committed). The repository's hooks
run it before every commit (`--staged`) and every push (the whole
tree); turn them on once per clone:

```sh
git config core.hooksPath .githooks
```

CI runs it too, on every pull request (the `tree rules + hygiene` job),
with the values read from the repository's secrets under the same keys
(`STAVE_LINUX_HOST`, `STAVE_AUTO_HOSTS`, …), and refuses any home
directory or agent worktree path whatever the configuration.
`scripts/release.sh` runs it before tagging.

Pick host aliases that are distinctive (not a crate or project name), so
the check has nothing to confuse them with.
