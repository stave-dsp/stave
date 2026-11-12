# Measure performance

How fast a plugin runs is measured, recorded and watched, but never
gated: a timing threshold on desktop hardware fails at random. This page
covers the perf report every plugin mounts, how to read a timing
honestly, the live CPU tap, and where to profile.

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

## Profile

The release perf report is a clean hot loop of exactly the plugin.
Attach a sampling profiler to it (Instruments, or `samply record`):

```sh
samply record cargo test --release -p <crate> --test checks perf -- --nocapture
```

The build is already tuned: the workspace's release profile uses fat
LTO with one codegen unit, and `.cargo/config.toml` targets
`apple-m1` on Apple Silicon (safe to distribute: every Apple Silicon
Mac is at least an M1). What remains is algorithmic; schedule it when a
measured need shows up.

`crates/stave-dsp/tests/datasheet.rs` prints the same kind of table for
every DSP primitive, so you can price a design before writing it:

```sh
cargo test --release -p stave-dsp --test datasheet -- --nocapture
```
