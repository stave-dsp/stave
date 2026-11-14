# Measure performance

How fast a plugin runs is measured, recorded and watched. One timing is
gated: a plugin's cost against the CPU budget it declares, in release
builds, on CPU time with the median of repeated runs. Everything else a
timing could say is reported, not gated, because a threshold nobody
chose fails at random. What CI gates is deterministic: the DSP
library's instruction counts. This page covers the budget and the perf
check every plugin mounts, how to read a timing honestly, the live CPU
tap and the stage cost taps, pricing a DSP stage, the instruction
counts CI holds the library to, and how to profile.

## The CPU budget

A plugin states what one channel may cost as
[`Plugin::CPU_BUDGET`](stave::Plugin::CPU_BUDGET), beside its identity
and layouts: CPU nanoseconds a sample at 48 kHz, in its worst parameter
state, with its own oversampling paid inside it.

```rust
use stave::Budget;

// Stave Tilt: two biquads a lane, about 6 ns measured.
const TILT: Budget = Budget::per_channel(40.0);
// Or as a share of one core's real time a channel (1 is the whole core).
const AMP: Budget = Budget::share_per_channel(0.05);

assert_eq!(Budget::REALTIME_NS.round(), 20_833.0); // one sample at 48 kHz
assert!((AMP.ns_per_sample().unwrap() - 1_041.7).abs() < 0.1);
assert!((TILT.share_of_realtime().unwrap() * 100.0 - 0.19).abs() < 0.01);
```

```text
impl Plugin for Tilt {
    const INFO: Info = Info { /* ... */ ..Info::EFFECT };
    /// Two biquads a lane; ~6 ns a sample a channel at full tilt.
    const CPU_BUDGET: Budget = Budget::per_channel(40.0);
    // ...
}
```

Per channel is the frame's cost over the larger of the main input and
output channel counts, so a stereo effect's frame may cost twice the
budget. The rate is 48 kHz whatever a host runs at: the number is a
property of the plugin, comparable across plugins and machines of one
kind.

**There is no default budget.** A plugin that declares none
([`Budget::UNDECLARED`](stave::Budget::UNDECLARED), the default) is
measured and reported against real time, and the check says
`NO BUDGET DECLARED ... reported, not checked`; nothing fails. A
framework-chosen fraction of real time would be wrong for nearly every
plugin: generous for an EQ, impossible for a circuit model oversampled
16×. The number is the author's statement of what the design costs,
with the reason beside it. Set it from the check's own report on your
machine, with the room a slower machine needs, and raise it only with a
reason.

## The perf check

[`plugin_checks!`](macro@stave_test::plugin_checks) mounts
[`perf_check`](stave_test::perf::perf_check) as the test
`perf_within_budget`. A project declares its representative
configurations as [`PerfCase`](stave_test::perf::PerfCase)s (the
default is one case at the default values):

```text
stave_test::plugin_checks!(my_plugin::MyPlugin, {
    perf: vec![PerfCase { name: "HEAVY", set: &[("Drive", 1.0)] }],
});
```

Each case is a fresh instance at 48 kHz with the case's values restored
before `prepare`, so an oversampling choice is in force from the first
sample. Effects are fed noise; instruments are timed sounding an
eight-note chord, released and struck again every half second so a
percussive voice is measured sounding rather than decayed to nothing
(struck once, a decaying instrument's runs spread by up to 19×). After a
warm-up, five timed runs of three seconds
each are measured in the thread's CPU time (time spent preempted is not
the plugin's), and the median is the case's cost. Where Linux's
performance counters can be read, the instructions a sample a channel
are printed too: the same on every run, so they see a change the clock
cannot.

The check adds a case of its own, `WORST KNOBS`: the worst parameter
state, searched rather than guessed. Starting from the costliest
declared case, each parameter but the bypass in turn is moved to each
candidate value (every step of a stepped parameter of up to 16 steps,
else its ends and middle) with the others where the search has left
them, in a short run, and kept where it cost most; the state the pass
ends in is measured in full. Parameters interact (a mode can switch off
the stage another knob loads), so each is judged in the others' state:
moving each alone from the defaults and combining the winners measured
cheaper than a declared case on four of the seventeen plugins. The
report prints the values it chose. The search is one pass of short runs,
so it can stop short of a state that only costs over time (a freeze
that fills a buffer); every declared case is held to the budget too, so
declare those.

The check skips debug builds, where optimisation levels misstate
release cost. Measure under release, and run it whenever you touch DSP:

```sh
cargo test --release -p <crate> --test checks perf -- --nocapture
```

It prints one row per case: the median nanoseconds a frame and a sample
a channel, that as a share of the budget (of real time when none is
declared), the instructions a sample a channel where counted, the
spread between the fastest and slowest run, and the costliest stage.
With a budget, a release run fails when any case's median is over it,
and the message names the case, its values and the hotspot:

```text
Stave Amp: over its CPU budget of 1041.7 ns a sample a channel at 48 kHz (5.00% of real time):
  WORST KNOBS costs 1388.2 ns (133% of budget, spread 2%) at Drive=10, Oversampling=3
    hotspot: preamp 2410 ns a sample (87% of the frame's 2776 ns over 2 channel(s)); then cab 302
```

The hotspot comes from the plugin's own stage cost taps (below): the
check averages each `<stage>_ns_per_sample` tap over a further untimed
run of the case. A plugin without cost taps is told to add them.

The measurement's defaults are its contract:

```rust
use stave_test::perf::PerfConfig;

let c = PerfConfig::default();
assert_eq!((c.sample_rate, c.block_size), (48_000.0, 256));
assert_eq!(c.warmup_blocks, 200);           // untimed, before the first run
assert_eq!((c.repeats, c.seconds_per_repeat), (5, 3.0)); // median of five 3 s runs
assert!(c.worst_case);                      // the WORST KNOBS search
assert_eq!((c.scan_warmup_blocks, c.scan_seconds), (40, 0.25)); // a candidate's run
assert_eq!(c.chord.len(), 8);               // instruments are timed sounding,
assert_eq!(c.retrigger_seconds, 0.5);       // the chord struck again this often
```

CI does not run the release check: a shared runner's timing is not the
machine the budget was stated for. Run it on your machine before a
change to a plugin's DSP lands.

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

The `cpu` tap is the whole plugin. To see where inside it the time
goes, wrap each stage's block in a [`CostTaps`](stave::CostTaps)
(`<stage>_ns_per_sample` and `<stage>_load`; the
[telemetry page](crate::concepts::telemetry) shows how). The perf check
names the costliest of them when a plugin is over budget, and
`stave-play` plots them live.


## Price a DSP stage

`crates/stave-dsp/tests/datasheet.rs` measures every DSP primitive the
same way, so you can price a design before writing it:

```sh
cargo test --release -p stave-dsp --test datasheet -- --nocapture
```

Its rows are committed as data, `crates/stave-dsp/datasheet.json`,
measured on one reference machine (`STAVE_REFERENCE_HOST` in your
[local configuration](crate::howto::local_config)) that the file describes, and the docs site's Cost reference page is generated from
it. Measuring while the site builds would publish a shared CI runner's
noise. The test fails, in debug too, where it times nothing and only
names each stage, whenever the stages it benches and the file's stages
differ. A new `bench(..)` line therefore lands with fresh data:

```sh
scripts/datasheet.sh          # on the reference machine, the file fetched back
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
scripts/profile.sh bench stage:coupler --seconds 10    # stave bench, any of its arguments
scripts/profile.sh -- path/to/binary --flag            # any command
```

On macOS it uses [samply](https://github.com/mstange/samply) (`cargo
install --locked samply`) and opens the Firefox Profiler, whose Flame
Graph tab is the flamegraph. On Linux it uses `perf`, plus
[inferno](https://github.com/jonhoo/inferno) for the SVG. The result is
in `target/profile`, and nothing opens when stdout is not a terminal, so
it works on a build host and fetches the profile back:

```sh
STAVE_OFFLOAD_FETCH=target/profile scripts/offload.sh macos -- scripts/profile.sh stage triode
samply load target/profile/stage-triode.json.gz
```

samply's profile carries its symbols in a `.syms.json` sidecar, so it
opens with function names on a machine that lacks the binaries. `perf`
needs `kernel.perf_event_paranoid` at 2 or below and the
`perf_event_open` syscall, which an unprivileged container does not
grant. The stave-dsp build container cannot profile, so profile on a
Mac, a Linux machine you own, or a GitHub runner. A plugin bundle built
by the bundle scripts has its symbols stripped. Profile a stage
through `stave bench stage:NAME`, which builds it in, or build the
plugin with `--profile profiling`.

The release perf check is also a clean hot loop of exactly the plugin,
so you can record it the same way:

```sh
scripts/profile.sh test <crate> checks perf
```

The build is already tuned: the workspace's release profile uses fat
LTO with one codegen unit, and `.cargo/config.toml` targets
`apple-m1` on Apple Silicon (safe to distribute: every Apple Silicon
Mac is at least an M1). What remains is algorithmic; schedule it when a
measured need shows up.
