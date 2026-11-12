# Your first plugin

Build a gain plugin from an empty crate in six steps: create the
crate, declare its parameters, process audio, glide instead of clicking,
export it as a VST3 and a CLAP plugin, and load it. Each step's code is complete, and
each runs: the checks under it drive the plugin in-process with
[`PluginHarness`](stave_test::PluginHarness) and assert what the step
promises.

## Step 1: Create the crate

A plugin is a library crate built as a `cdylib` (what hosts load) and an
`rlib` (what its tests link). It depends on [`stave`](stave) alone; tests
add [`stave-test`](stave_test). Inside a checkout of the Stave
repository, create `examples/gain/` and add it to the workspace's
`members` in the root `Cargo.toml`:

```toml
# examples/gain/Cargo.toml
[package]
name = "gain"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
stave = { path = "../../crates/stave" }

[dev-dependencies]
stave-test = { path = "../../crates/stave-test" }
```

`src/lib.rs` starts with one import. Everything a plugin's source needs
is in the prelude; needing more is a framework bug, not an import to add.

```
use stave::prelude::*;
```

## Step 2: Declare the parameters

Parameters are fields of one struct that derives [`Params`](stave_macros::Params),
built in its `Default`. Each gets a [`ParamId`](stave::ParamId) that is
forever (hosts store automation and sessions against it), a name, a
range and a unit. Bypass is declared, never implemented: the framework
runs it.

```
use stave::prelude::*;

#[derive(Params)]
pub struct GainParams {
    pub gain: FloatParam,
    pub bypass: BoolParam,
}

impl Default for GainParams {
    fn default() -> Self {
        GainParams {
            gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0)).unit(Unit::Db),
            bypass: BoolParam::bypass(ParamId(2), "Bypass"),
        }
    }
}

let params = GainParams::default();
assert_eq!(params.gain.value(), 0.0); // with no `.default(..)`: the value nearest 0
assert_eq!(params.gain.format(-6.0).as_str(), "-6.0 dB");
assert_eq!(params.count(), 2);
assert_eq!(Params::bypass(&params).unwrap().meta().name, "Bypass");
```

## Step 3: Process audio

The plugin is a type that implements [`Plugin`](stave::Plugin). `INFO`
names it to hosts; its `id` is forever too. `new` keeps the shared
params; `process` runs once per block on the audio thread.

`block.lanes::<2>()` is the per-frame loop over up to two lanes: it
counts the live lanes, zeroes any output it does not drive, and hands
the body one frame at a time. This first version reads the gain once
per block with `value()`.

```
use stave::prelude::*;
use stave_test::PluginHarness;
#
# #[derive(Params)]
# pub struct GainParams {
#     pub gain: FloatParam,
#     pub bypass: BoolParam,
# }
#
# impl Default for GainParams {
#     fn default() -> Self {
#         GainParams {
#             gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0)).unit(Unit::Db),
#             bypass: BoolParam::bypass(ParamId(2), "Bypass"),
#         }
#     }
# }

pub struct Gain {
    params: Arc<GainParams>,
}

impl Plugin for Gain {
    const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
    type Params = GainParams;

    fn new(params: &Arc<GainParams>, _telemetry: &Telemetry) -> Self {
        Gain { params: params.clone() }
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let g = db_to_amp(self.params.gain.value());
        block.lanes::<2>().run(|frame| {
            for lane in 0..frame.lanes() {
                frame.write(lane, frame.input(lane) * g);
            }
        });
    }
}

// +6 dB is very nearly twice the amplitude (exactly twice is 6.02 dB).
let mut h = PluginHarness::<Gain>::builder().set("Gain", 6.0).build();
h.inputs_mut().for_each(|lane| lane.fill(0.25));
h.process(&[]);
for lane in h.outputs() {
    assert!(lane.iter().all(|&y| (y - 0.25 * db_to_amp(6.0)).abs() < 1e-7));
    assert!((lane[0] - 0.5).abs() < 0.002);
}
```

It works, and it clicks: a host that moves the gain from 0 dB to −24 dB
moves every sample of the next block at once.

## Step 4: Glide instead of clicking

Declare the glide on the param, `.smooth(Smooth::ms(10.0))`, and let a
[`Smoother`](stave::params::Smoother) run it. Build the smoother from
the param in `new`, `prepare` it with the sample rate, and take one
ramp per block; `next()` gives each frame's value.

```
use stave::prelude::*;
use stave_test::{change, PluginHarness};

#[derive(Params)]
pub struct GainParams {
    pub gain: FloatParam,
    pub bypass: BoolParam,
}

impl Default for GainParams {
    fn default() -> Self {
        GainParams {
            gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(10.0)),
            bypass: BoolParam::bypass(ParamId(2), "Bypass"),
        }
    }
}

pub struct Gain {
    params: Arc<GainParams>,
    gain: Smoother,
}

impl Plugin for Gain {
    const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
    type Params = GainParams;

    fn new(params: &Arc<GainParams>, _telemetry: &Telemetry) -> Self {
        Gain { params: params.clone(), gain: Smoother::new(&params.gain) }
    }

    fn prepare(&mut self, env: &Environment) {
        self.gain.prepare(env);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut gain = self.gain.ramp(block, &self.params.gain);
        block.lanes::<2>().run(|frame| {
            let g = db_to_amp(gain.next());
            for lane in 0..frame.lanes() {
                frame.write(lane, frame.input(lane) * g);
            }
        });
    }
}

let mut h = PluginHarness::<Gain>::new(48_000.0, 256);
h.inputs_mut().for_each(|lane| lane.fill(0.25));
h.process(&[]); // the first block after `prepare` starts at the value, gliding nowhere

// A host moves the knob: the next block glides down, it does not jump.
h.set("Gain", -24.0);
h.process(&[]);
let out = &h.outputs()[0];
assert!(out.windows(2).all(|w| w[1] < w[0]), "falls every frame");
assert!(out[0] > 0.24, "starts near where it was");
assert!(out[255] > 0.25 * db_to_amp(-24.0), "still on its way");

// Ten time constants later it has arrived.
for _ in 0..19 {
    h.process(&[]);
}
assert!((h.outputs()[0][255] - 0.25 * db_to_amp(-24.0)).abs() < 1e-5);

// Automation lands on its frame: nothing moves before frame 128.
let mut h = PluginHarness::<Gain>::new(48_000.0, 256);
h.inputs_mut().for_each(|lane| lane.fill(0.25));
h.process_with(&[change(ParamId(1), 128, -24.0)], &[]);
let out = &h.outputs()[0];
assert!(out[..128].iter().all(|&y| y == 0.25));
assert!(out[128] < 0.25);
```

A session load is not a knob move: a value the host restores is jumped
to, never glided to, so a saved mix never fades in. The harness's
builder starts params that way:

```
# use stave::prelude::*;
# use stave_test::PluginHarness;
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
let mut h = PluginHarness::<Gain>::builder().set("Gain", -24.0).build();
h.inputs_mut().for_each(|lane| lane.fill(0.25));
h.process(&[]);
assert!(h.outputs()[0].iter().all(|&y| y == 0.25 * db_to_amp(-24.0)));
```

## Step 5: Export it

One line at the end of `lib.rs` makes the crate a VST3 and a CLAP plugin
at once. Its class ids derive from `INFO.id`;
[`stave::identity`] reads them, so a test can pin them and a change that
would orphan every saved session fails the build.

```
# use stave::prelude::*;
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
stave::export!(Gain);

let id = stave::identity::<Gain>();
assert_eq!(id.clap, "com.example.gain");
assert_ne!(id.vst3_component, id.vst3_controller);
```

The framework now owns everything around `process`: the click-free,
latency-matched bypass, automation and state, the denormal backstop, and
panics at the host boundary (see [the lifecycle](crate::concepts::lifecycle)).

## Step 6: Build it and load it

From the repository root, build, bundle, validate and install both
formats in one command, then open the plugin in Stave's host:

```sh
bash scripts/install.sh gain release
cargo run --release -p stave-play -- build/Gain.vst3
```

The bundles are `build/Gain.vst3` and `build/Gain.clap`, installed into
your per-user plug-in folders (see [Getting started](crate::getting_started)),
where a DAW finds them on its next scan.

## Next

The plugin has no face; a host shows its generic controls.
[A face and meters](crate::tutorials::face_and_meters) gives it one, and
[Prove it](crate::tutorials::prove_it) gives it the tests that make it
trustworthy.
