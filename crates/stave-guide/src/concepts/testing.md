# Testing a plugin

A Stave plugin is tested in-process, as Rust: [`PluginHarness`](stave_test::PluginHarness)
drives the real [`Plugin`](stave::Plugin) block by block with no bundle and
no FFI, [`FaceDriver`](stave_test::FaceDriver) drives its face the way a
person's hands do, and one line of
[`plugin_checks!`](macro@stave_test::plugin_checks) mounts every check a
plugin owes. The generic checks prove a plugin is *sane*; only the
plugin's own property tests prove it is *right*.

## Every project states its own properties

Tilt, the worked example, is the cautionary tale. Its first build shipped
with the knob backwards — turning it up darkened the sound — and passed
the validation suite, the no-allocation check, the face checks and both
industry validators, because no generic check can know which way a knob
should point. A thirty-second sine measurement caught it, and
`examples/tilt/tests/properties.rs` now makes the mistake unshippable.

A property test states a promise the design makes and derives the
expected value — from the math, the physics or the declaration — instead
of recording whatever a run produced. Its name reads as the claim, its
tolerances carry the measured values they protect, and a project without
its own is not done. Write the first one before the DSP.

```
use stave_test::PluginHarness;
use stave_tilt_vst3::Tilt;

/// Steady-state gain in dB of a sine through Tilt at `tilt` dB.
fn gain_db(hz: f64, tilt: f64) -> f64 {
    use stave_measure::signal::{Signal, Sine};
    let rate = 48_000.0;
    let mut h = PluginHarness::<Tilt>::builder().sample_rate(rate as f32).set("Tilt", tilt).build();
    let sine = Sine { frequency_hz: hz, amplitude: 0.1, sample_rate: rate };
    let input = sine.samples(0, 9_600);
    for ch in h.inputs_mut() {
        ch.fill(0.0);
    }
    let mut out = Vec::new();
    for chunk in input.chunks(256) {
        for ch in h.inputs_mut() {
            ch[..chunk.len()].copy_from_slice(chunk);
        }
        h.process(&[]);
        out.extend_from_slice(&h.outputs()[0]);
    }
    // Skip the settling, compare levels.
    let level = stave_measure::level::rms;
    stave_measure::level::db(level(&out[4_800..]) / level(&input[4_800..]))
}

// Positive tilt is brighter: the highs gain what the lows lose.
assert!(gain_db(5_000.0, 6.0) > 5.0);
assert!(gain_db(100.0, 6.0) < -5.0);
```

[`stave_measure`] holds the measurements as pure functions over buffers —
levels, spectra, THD, latency, transfer functions, aliasing and
intermodulation, dynamics, modulation, zipper noise, reverb tails — each
stating the assumptions it measures under (bin-centred tones, settled
systems). Plugin tests, the host's bench and the DSP conformance suites
all measure through it.

## PluginHarness

[`PluginHarness`](stave_test::PluginHarness) owns a plugin, its
parameters, its telemetry and fixed-size buffers, and is built prepared.
The [builder](stave_test::HarnessBuilder) chooses the sample rate
(default 48 kHz), the block size (default 256 frames), one of the
plugin's [`LAYOUTS`](stave::Plugin::LAYOUTS) and starting values, which
are restored before `prepare` so nothing glides in.

It supplies everything a host does: input, sidechain
([`aux_mut`](stave_test::PluginHarness::aux_mut)) and output buffers, a
transport, note events, and sample-accurate parameter changes through
[`process_with`](stave_test::PluginHarness::process_with) and
[`change`](stave_test::change). Short and empty blocks go through
[`process_frames`](stave_test::PluginHarness::process_frames), and
[`save`](stave_test::PluginHarness::save) /
[`load`](stave_test::PluginHarness::load) are the session's state, as a
host stores it.

```
use stave::ParamId;
use stave_test::{change, PluginHarness};
use stave_tilt_vst3::Tilt;

let mut h = PluginHarness::<Tilt>::builder().block_size(64).build();
assert_eq!(h.sample_rate(), 48_000.0);  // the default rate
h.inputs_mut().for_each(|c| c.fill(0.5));

// Output (id 2) jumps to −24 dB at frame 32 of this block: sample accurate.
h.process_with(&[change(ParamId(2), 32, -24.0)], &[]);
assert_eq!(h.value("Output"), -24.0);   // the table holds the last value

// State round-trips into a fresh instance.
let saved = h.save();
let mut fresh = PluginHarness::<Tilt>::new(48_000.0, 64);
fresh.load(&saved).unwrap();
assert_eq!(fresh.value("Output"), -24.0);
```

The harness is also a `stave-host`
[`Instance`](stave_host::instance::Instance), the seam every host-side
measurement is written against. The bench phases, the frequency response,
the validation checks and stave-host's block driver run on a linked
plugin exactly as they run on a loaded VST3 —
[`run`](stave_test::PluginHarness::run) is that driver's capture, so
there is one block loop in the whole stack.

```
use stave_host::instance::Instance;
use stave_test::PluginHarness;
use stave_tilt_vst3::Tilt;

let h = PluginHarness::<Tilt>::new(48_000.0, 256);
let params = Instance::params(&h);
assert_eq!(params[0].name, "Tilt");
assert_eq!((params[0].min, params[0].max), (-6.0, 6.0));
```

## FaceDriver

[`FaceDriver`](stave_test::FaceDriver) is a face under a test's hands:
the face's own runtime over a real controller, headless and
deterministic. It clicks, drags, hovers, scrolls and types at parts by
name (a bound parameter's name, a role, a `node.id`) or at exact points,
then reads back parameter values, the edits sent to the host, hover,
focus, the cursor and every line of text on the face. Its
[`trace`](stave_test::FaceDriver::trace) replays the session.

```
use stave_test::face::{dy, FaceDriver, Key};
use stave_tilt_vst3::Tilt;

let mut ui = FaceDriver::of::<Tilt>();
ui.press("tilt").move_to(dy(-55.0));          // a quarter of the travel up
assert_eq!(ui.value("tilt"), 3.0);
assert!(ui.shows("3.0 dB"));                  // the readout, while dragging
ui.release().exit();
assert!(!ui.shows("3.0 dB"));                 // and gone after

ui.click("output").key(Key::End);             // focus, then the top of the range
assert_eq!(ui.value("output"), 24.0);
ui.assert_bracketed();                        // every edit was begin → perform → end
```

### Golden pictures

When the look matters, compare it with
[`assert_golden!`](macro@stave_test::assert_golden): the driver draws the
face with [`BoxText`](stave_raster::BoxText) metrics, the same on every
machine, and the macro compares it within a tolerance to
`tests/golden/<name>.png` beside the tests. On a mismatch it writes the
actual picture and a diff under `target/golden-diff`.
`STAVE_BLESS=1 cargo test` rewrites the goldens from what the code draws
now; review them like any other change. `scripts/check-golden-sizes.sh`
holds every committed golden in the tree to a 3 MiB budget, so draw them
at 1×.

## plugin_checks!

Every plugin owes the same checks, and
[`plugin_checks!`](macro@stave_test::plugin_checks) mounts them from one
line in a test file of its own (the macro installs the no-allocation
guard as the file's global allocator):

```
// tests/checks.rs
stave_test::plugin_checks!(stave_tilt_vst3::Tilt, {
    perf: vec![PerfCase { name: "FULL TILT", set: &[("Tilt", 6.0)] }],
});
# fn main() {}
```

The [`Config`](stave_test::plugin_checks::Config) fields are all
optional: `perf` (the perf check's cases; default, the default values;
the searched worst parameter state is always added),
`off_face` (parameters no grab on the face moves, such as host-only
settings) and `seeds` (for random automation and face sessions; default
`0..4`). Each check is its own test:

| test | what it holds |
|---|---|
| `passes_validation` | stave-host's validation suite, through the plugin's own VST3 entry points, so the ABI is under test too |
| `process_never_allocates` | the whole `process` loop under a guard that aborts on any allocation, while every parameter sweeps its range (debug builds) |
| `perf_within_budget` | CPU ns a sample a channel at 48 kHz in each case and the searched worst parameter state, against the plugin's [`CPU_BUDGET`](stave::Plugin::CPU_BUDGET): fails over it, naming the hotspot stage; reports only when none is declared (release builds; debug builds skip it; see [Measure performance](crate::howto::performance)) |
| `face_binds_resolve` | every bound param exists under the name the face was built with, and stepped gestures bind stepped params |
| `face_grabs_land` | parts sit inside the face, and a press at each grab's centre grabs that part |
| `face_keeps_the_interaction_laws` | the gesture, layout, overlay and random-session laws on every page and step |
| `any_block_size` | blocks of 0, 1, odd and full sizes, and a re-prepare to a larger maximum: output stays finite |
| `any_sample_rate` | 8 kHz to 768 kHz, both rate families, in a changing order: output stays finite |
| `prepare_and_reset_cycles` | prepare, reset and process interleaved; after a reset the plugin sounds as it did fresh |
| `random_automation` | every param moved at random every block, smoothed and sample-accurate, for each seed |
| `state_round_trip` | random values saved and loaded into a fresh instance come back, and both sound the same |
| `deterministic` | two fresh instances fed the same input give bit-identical output |

The face checks pass trivially for a plugin without a face. The checks
are functions too — [`stave_test::checks`], [`stave_test::face::laws`],
[`stave_test::no_alloc`] — for a test that needs one on its own terms.

### The validation suite

`passes_validation` runs the same suite `stave-bench --validate` runs on
any VST3. Each check gets fresh instances; only the unambiguous fails,
and a judgment call is reported as `info` with its numbers:

- `reproducible`: two instances, the same input, bit-identical output;
- `no_nan_inf`: no NaN or infinity anywhere in the output;
- `param_sweep`: every sweepable control to its extremes while processing;
- `params_glide`: a host move of a continuous param glides rather than
  steps (a step is a click);
- `self_noise`: silent input stays below a floor (no hiss, hum, DC or
  denormal storm);
- `silence_tail`: the output goes quiet after the input stops;
- `deadline_headroom`: p99 block cost against the realtime budget (a
  miss fails a release build; in a debug build it is reported, not failed);
- `state_roundtrip`: params nudged off their defaults, saved, loaded into
  a fresh instance and saved again: the same bytes;
- `solver_converges`: every param held at each end of its range while a
  stimulus plays, and no circuit solve ends at its iteration cap;
- `channels_linked` and `channels_split`: identical input on every
  channel gives identical output unless the plugin means to differ, and
  linked channels stay close when their inputs differ slightly;
- `bypass_honest`: an effect's bypassed output is its latency-delayed dry
  input, bit for bit; an instrument's is silence.

## Performance

The perf check gates only what the plugin states: its
[`CPU_BUDGET`](stave::Plugin::CPU_BUDGET), ns a sample a channel at
48 kHz. There is no default threshold, because one the author did not
choose cries wolf; undeclared, the check reports and never fails. It
warms up, repeats the timed runs in CPU time, takes the median, searches
the parameters for the costliest state, and prints each case per
channel beside the budget (or real time, 20 833 ns a sample at 48 kHz)
with the spread, so a noisy run discredits itself; over the budget it
names the stage that took the time. Measure it in release when touching
DSP:

```sh
cargo test --release -p stave-tilt-vst3 --test checks perf -- --nocapture
```

Record the baselines in the test file's header, as Tilt's
`tests/checks.rs` does, and update them when they legitimately move.
Single runs on a desktop carry 5–8% noise; never conclude from one.

## Stability

Recirculating systems (feedback delays, resonators, meshes) can pump:
energy that compounds slowly enough to pass a short test.
[`stave_test::stability`] drives a system with a resonant tone at torture
settings and asserts the energy *trend* over long windows far apart is
flat ([`assert_no_pump`](stave_test::assert_no_pump)).

## Where tests live

A plugin's own tests live in its `tests/`: `checks.rs` (the macro),
`properties.rs` (its promises), `editor.rs` (its face's promises through
the driver) and `golden.rs` if the look matters. Shared machinery lives in
the `stave-test` library; `crates/stave-test/tests/` holds the framework's
own conformance suites, which test the DSP vocabulary, not your plugin.
