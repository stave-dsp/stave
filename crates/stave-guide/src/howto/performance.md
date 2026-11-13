# Measure performance

How fast a plugin runs is measured, recorded and watched, but its time
is never gated: a timing threshold on desktop hardware fails at random.
What is gated is deterministic: the DSP library's instruction counts.
This page covers the perf report every plugin mounts, how to read a
timing honestly, the live CPU tap, pricing a DSP stage, the instruction
counts CI holds the library to, and how to profile.

## The perf report

[`plugin_checks!`](macro@stave_test::plugin_checks) mounts
[`perf_report`](stave_test::perf::perf_report) as the test
`perf_report_for_the_record`. A project declares its representative
configurations as [`PerfCase`](stave_test::perf::PerfCase)s (the
default is one case at the default values):

```text
stave_test::plugin_checks!(my_plugin::MyPlugin, {
    perf: vec![PerfCase { name: "HEAVY", set: &[("Drive", 1.0)] }],
});
```

The report skips debug builds, where optimisation levels misstate
release cost. Measure under release, and run it whenever you touch DSP:

```sh
cargo test --release -p <crate> --test checks perf -- --nocapture
```

It prints one row per case: the median nanoseconds per frame, that
figure as a percentage of the realtime budget, and the spread between
the fastest and slowest run. It asserts nothing; the printed table is
the record, and a project keeps its baselines in the header of
`tests/checks.rs` (Tilt's records about 4 ns/frame flat and 12 at full
tilt), updated when they legitimately move.

The measurement follows the noise doctrine below. These defaults are
its contract:

```rust
use stave_test::perf::PerfConfig;

let c = PerfConfig::default();
assert_eq!((c.sample_rate, c.block_size), (48_000.0, 256));
assert_eq!(c.warmup_blocks, 200);           // untimed, before the first run
assert_eq!((c.repeats, c.seconds_per_repeat), (5, 3.0)); // median of five 3 s runs
assert_eq!(c.chord.len(), 8);               // instruments are timed sounding
// The budget a row is a percentage of: one frame at 48 kHz.
assert_eq!((1e9 / f64::from(c.sample_rate)).round(), 20_833.0);
```

## Read timings honestly

A single run's frame timing on a desktop carries five to eight percent
of noise. Warm up, repeat, take the median, and report the spread;
never conclude from one run. A spread wider than the change you are
looking for means the measurement cannot see it.

## The CPU tap

Both format wrappers time every `process` call and publish the plugin's
time over the block's realtime budget (`0..1`, above 1 when it missed)
as the wrapper tap `cpu`, beside the input and output peaks. Any tool
reads it by id. `stave-play`'s Perf tab shows it as the plugin's inner
time next to the host's outer time, so the gap is wrapper and host
overhead, and adds a load track against the budget, deadline misses and
a one-click sampled flamegraph.

```rust
use stave_core::runtime::WrapperTaps;
use stave_core::TapId;

assert_eq!(WrapperTaps::CPU, TapId::new("cpu"));
```


## Price a DSP stage

`crates/stave-dsp/tests/datasheet.rs` measures every DSP primitive the
same way, so you can price a design before writing it:

```sh
cargo test --release -p stave-dsp --test datasheet -- --nocapture
```

Its rows are committed as data, `crates/stave-dsp/datasheet.json`,
measured on one reference machine (stave-mini, an Apple M1) that the
file names, and the docs site's Cost reference page is generated from
it. Measuring while the site builds would publish a shared CI runner's
noise. The test fails, in debug too, where it times nothing and only
names each stage, whenever the stages it benches and the file's stages
differ. A new `bench(..)` line therefore lands with fresh data:

```sh
scripts/datasheet.sh          # on the reference machine (ssh mini), the file fetched back
scripts/datasheet.sh local    # on this machine, to explore (don't commit those numbers)
```

## Instruction counts in CI

Wall clock on a shared runner cannot see a 5% regression, but an
instruction count can, because it is the same on every run.
`crates/stave-dsp/benches/stages.rs` is a harness of representative
stages: the biquad, the SVF, the oversampler, the FFT, three
convolvers, the triode stage, the diode clipper and a plugin-sized amp
chain. `scripts/icount.sh` runs each under valgrind's cachegrind for two
lengths and takes the difference, so set-up cancels and what is left is
the per-sample cost. CI's `perf` workflow runs it on every change to
stave-dsp or stave-core and fails when a stage costs more than 5% over
the committed baseline, `crates/stave-dsp/benches/stages.icount`.
The workflow is path-filtered, so it is not a required check.

```sh
scripts/icount.sh                  # check against the baseline (Linux, valgrind)
scripts/icount.sh check triode     # one stage
scripts/icount.sh bless            # move the baseline, on purpose
```

The counts depend on rustc, valgrind, glibc and the CPU family, so bless
where CI counts: x86_64 Ubuntu 24.04 with valgrind 3.22 (the stave-dsp
build container matches). Every CI run uploads its counts as the
`icount` artifact, a baseline ready to commit. Bless when a stage is
meant to cost more, and when the check reports a drop beyond the
threshold, so the gain is locked in. To cover a new stage, add an
`Entry` to the harness and bless.

## Profile

`scripts/profile.sh` records a flamegraph of a harness stage, a test or
a plugin bench, built in the `profiling` profile (release with line
tables and symbols):

```sh
scripts/profile.sh stage amp-chain                     # a stage, 10 s (--seconds N)
scripts/profile.sh test stave-dsp datasheet primitive_datasheet
scripts/profile.sh bench stage:coupler --seconds 10    # stave-bench, any of its arguments
scripts/profile.sh -- path/to/binary --flag            # any command
```

On macOS it uses [samply](https://github.com/mstange/samply) (`cargo
install --locked samply`) and opens the Firefox Profiler, whose Flame
Graph tab is the flamegraph. On Linux it uses `perf`, plus
[inferno](https://github.com/jonhoo/inferno) for the SVG. The result is
in `target/profile`, and nothing opens when stdout is not a terminal, so
it works on a build host and fetches the profile back:

```sh
STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh mini -- scripts/profile.sh stage triode
samply load target/profile/stage-triode.json.gz
```

samply's profile carries its symbols in a `.syms.json` sidecar, so it
opens with function names on a machine that lacks the binaries. `perf`
needs `kernel.perf_event_paranoid` at 2 or below and the
`perf_event_open` syscall, which an unprivileged container does not
grant. The stave-dsp build container cannot profile, so profile on a
Mac, a Linux machine you own, or a GitHub runner. A plugin bundle built
by the bundle scripts has its symbols stripped. Profile a stage
through `stave-bench stage:NAME`, which builds it in, or build the
plugin with `--profile profiling`.

The release perf report is also a clean hot loop of exactly the plugin,
so you can record it the same way:

```sh
scripts/profile.sh test <crate> checks perf
```

The build is already tuned: the workspace's release profile uses fat
LTO with one codegen unit, and `.cargo/config.toml` targets
`apple-m1` on Apple Silicon (safe to distribute: every Apple Silicon
Mac is at least an M1). What remains is algorithmic; schedule it when a
measured need shows up.
