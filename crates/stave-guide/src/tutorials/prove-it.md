# Prove it

Generic checks prove a plugin is sane: it loads, it never allocates on
the audio thread, its state survives a round trip. Only the plugin's
own properties prove it is right. Write them first, measure with
`stave-measure`, then mount every generic check with one line of
`stave_test::plugin_checks!`.

## Why a plugin needs its own properties

Stave Tilt's first build shipped with its one knob backwards: turning it
toward "brighter" made the sound darker. It passed every generic check,
because no generic check can know which way a knob should point. A
measurement caught it, and `examples/tilt/tests/properties.rs` now makes
it unshippable. That file states three promises only Tilt can make:

- **The sign.** At +6 dB tilt the highs gain what the lows lose; at −6
  dB the curve mirrors.
- **The pivot holds still.** At full tilt, 650 Hz moves less than
  0.1 dB.
- **Zero tilt is bit-flat.** A tilt of 0 is a true bypass, sample for
  sample, not a recursion that happens to have unity gain.

A property test states a promise the design makes, and its expected
value is derived (from physics, from the maths, from the declaration),
never recorded from a run: that is what separates it from a snapshot,
which freezes whatever the code happened to do. Its name reads as the
claim, and its tolerances carry the measured values they protect.

Here is the sign property, run against the real Tilt:

```
use stave_measure::level::{db, rms};
use stave_test::PluginHarness;
use stave_tilt_vst3::Tilt;

const RATE: f32 = 48_000.0;

/// Steady-state gain (dB) of a sine at `hz` through Tilt at `tilt` dB.
fn gain_db(hz: f32, tilt: f64) -> f64 {
    let mut h = PluginHarness::<Tilt>::builder().sample_rate(RATE).set("Tilt", tilt).build();
    let mut out = Vec::new();
    let mut input = Vec::new();
    for block in 0..96 {
        let phase0 = block * 256;
        let chunk: Vec<f32> = (0..256)
            .map(|i| 0.1 * (std::f32::consts::TAU * hz * (phase0 + i) as f32 / RATE).sin())
            .collect();
        h.inputs_mut().for_each(|lane| lane.copy_from_slice(&chunk));
        h.process(&[]);
        if block >= 48 {
            // past the filters' settling
            input.extend_from_slice(&chunk);
            out.extend_from_slice(&h.outputs()[0]);
        }
    }
    db(rms(&out) / rms(&input))
}

// Positive tilt is brighter: the highs rise, the lows fall.
assert!(gain_db(5_000.0, 6.0) > 5.0 && gain_db(100.0, 6.0) < -5.0);
// And the knob mirrors.
assert!(gain_db(100.0, -6.0) > 5.0 && gain_db(5_000.0, -6.0) < -5.0);
// The pivot holds still.
assert!(gain_db(650.0, 6.0).abs() < 0.1);
```

## Properties for the gain plugin

The gain plugin from [Your first plugin](crate::tutorials::first_plugin)
makes two promises of its own: the measured gain is the knob's value,
and 0 dB is exact.

```
use stave::prelude::*;
use stave_measure::level::{db, rms};
use stave_test::PluginHarness;
# #[derive(Params)]
# pub struct GainParams { pub gain: FloatParam, pub bypass: BoolParam }
# impl Default for GainParams {
#     fn default() -> Self {
#         GainParams {
#             gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0)).unit(Unit::Db).smooth(Smooth::ms(10.0)),
#             bypass: BoolParam::bypass(ParamId(2), "Bypass"),
#         }
#     }
# }
# pub struct Gain { params: Arc<GainParams>, gain: Smoother }
# impl Plugin for Gain {
#     const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
#     type Params = GainParams;
#     fn new(params: &Arc<GainParams>, _telemetry: &Telemetry) -> Self {
#         Gain { params: params.clone(), gain: Smoother::new(&params.gain) }
#     }
#     fn prepare(&mut self, env: &Environment) { self.gain.prepare(env); }
#     fn process(&mut self, block: &mut Block<'_>) {
#         let mut gain = self.gain.ramp(block, &self.params.gain);
#         block.lanes::<2>().run(|frame| {
#             let g = db_to_amp(gain.next());
#             for lane in 0..frame.lanes() { frame.write(lane, frame.input(lane) * g); }
#         });
#     }
# }

fn noise(n: usize) -> Vec<f32> {
    let mut x = 0x2545_f491_u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 - 0.5) * 0.5
        })
        .collect()
}

/// The knob is what you hear: the measured gain is the knob's dB.
fn the_knob_is_the_gain() {
    let input = noise(256);
    for knob in [-24.0, -6.0, 0.0, 3.5, 24.0] {
        let mut h = PluginHarness::<Gain>::builder().set("Gain", knob).build();
        h.input_mut(0).copy_from_slice(&input);
        h.process(&[]);
        let measured = db(rms(&h.outputs()[0]) / rms(&input));
        assert!((measured - knob).abs() < 1e-4, "knob {knob} dB measured {measured:.5} dB");
    }
}

/// 0 dB is the identity, bit for bit.
fn zero_db_is_bit_exact() {
    let input = noise(256);
    let mut h = PluginHarness::<Gain>::new(48_000.0, 256);
    h.input_mut(0).copy_from_slice(&input);
    h.process(&[]);
    assert!(input.iter().zip(&*h.outputs()[0]).all(|(x, y)| x.to_bits() == y.to_bits()));
}

the_knob_is_the_gain();
zero_db_is_bit_exact();
```

In the plugin's crate these are `#[test]` functions in
`tests/properties.rs`, run by `cargo test`.

## Every generic check, in one line

`stave_test::plugin_checks!` mounts every check a plugin owes, each as
its own test. It goes in a test file of its own, `tests/checks.rs`,
because it installs the no-allocation guard as that file's global
allocator:

```
// tests/checks.rs
stave_test::plugin_checks!(stave_tilt_vst3::Tilt, {
    perf: vec![PerfCase { name: "FLAT", set: &[("Tilt", 0.0)] }, PerfCase { name: "FULL TILT", set: &[("Tilt", 6.0)] }],
});
# fn main() {}
```

The plugin's path is `crate_name::Type`. Every field of the optional
configuration has a default: `perf` (the perf report's cases; default,
the default values), `off_face` (names of params no part of the face
moves, so the face's laws do not expect a grab for them) and `seeds`
(for random automation and face sessions; default `0..4`).

It mounts:

- **`passes_validation`**: stave-host's validation suite, run through
  the plugin's own VST3 entry points, so the ABI is under test too.
- **`process_never_allocates`** (debug builds): `process` under an
  allocator that aborts, while every parameter moves; no list of params
  to keep up to date.
- **`perf_report_for_the_record`**: the perf report for each case
  (it measures in release builds and documents; it never fails a run).
- **`face_binds_resolve`**, **`face_grabs_land`** and
  **`face_keeps_the_interaction_laws`** (when the plugin has a face):
  every bind names a real param of the right kind, a press at each
  grab's centre grabs that part, and the interaction laws hold on every
  page and step, including seeded random sessions.
- **`any_block_size`**, **`any_sample_rate`**,
  **`prepare_and_reset_cycles`**, **`random_automation`**,
  **`state_round_trip`** and **`deterministic`**: the host behaviours
  pluginval and clap-validator probe. Blocks of 0, 1, odd and full
  frame counts, then a re-prepare to larger blocks; rates from 8 kHz to 768 kHz; a reset that sounds like a
  fresh instance; seeded automation, smoothed and sample-accurate; state
  that comes back and sounds the same; two instances that agree bit for
  bit.

The host-behaviour checks are ordinary functions in
[`stave_test::checks`], each over a `PluginHarness`, so any test can
call one directly:

```
use stave_tilt_vst3::Tilt;

stave_test::checks::state_round_trip::<Tilt>();
stave_test::checks::deterministic::<Tilt>();
```

Run everything with `cargo test -p <crate>`; the perf report measures
under `cargo test --release -p <crate> --test checks perf -- --nocapture`.
For a gain plugin in the workspace that is:

```sh
cargo test -p gain
cargo test --release -p gain --test checks perf -- --nocapture
```

Testing in depth, from the harness to the face's laws and the
industry validators, is in [Testing](crate::concepts::testing).
