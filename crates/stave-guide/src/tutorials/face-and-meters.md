# A face and meters

Give the gain plugin from [Your first plugin](crate::tutorials::first_plugin)
an editor and an output meter: publish the output level on a typed tap,
declare a face of parts on a theme, and test the face the way a person
uses it, with [`FaceDriver`](stave_test::FaceDriver), headless and
deterministic.

## Step 1: Publish a tap

A face never reaches into the plugin. What the plugin wants to show, it
publishes on [`Telemetry`](stave::Telemetry), the lock-free channel from
the plugin to faces and tools, as typed taps. Declare them as a struct
under [`#[derive(Taps)]`](stave_macros::Taps): each field gets a
[`TapId`](stave::TapId) constant named after it (`level` becomes
`GainTaps::LEVEL`), so the plugin and its face name the same constant
and a typo does not compile.

Build the taps in `new` from the telemetry the framework hands it, then
write them in `process`. `run` returns the block's [`Peaks`](stave::Peaks),
measured as the frames were written, so the output level costs nothing
extra:

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
#             gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0))
#                 .unit(Unit::Db)
#                 .smooth(Smooth::ms(10.0)),
#             bypass: BoolParam::bypass(ParamId(2), "Bypass"),
#         }
#     }
# }

#[derive(Taps)]
pub struct GainTaps {
    /// The block's output peak, linear (1 = 0 dBFS).
    pub level: Tap,
}

pub struct Gain {
    params: Arc<GainParams>,
    gain: Smoother,
    taps: GainTaps,
}

impl Plugin for Gain {
    const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
    type Params = GainParams;

    fn new(params: &Arc<GainParams>, telemetry: &Telemetry) -> Self {
        Gain { params: params.clone(), gain: Smoother::new(&params.gain), taps: GainTaps::new(telemetry) }
    }

    fn prepare(&mut self, env: &Environment) {
        self.gain.prepare(env);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut gain = self.gain.ramp(block, &self.params.gain);
        let peaks = block.lanes::<2>().run(|frame| {
            let g = db_to_amp(gain.next());
            for lane in 0..frame.lanes() {
                frame.write(lane, frame.input(lane) * g);
            }
        });
        self.taps.level.set(f64::from(peaks.output));
    }
}

assert_eq!(GainTaps::LEVEL.name(), "level");
let mut h = PluginHarness::<Gain>::builder().set("Gain", 6.0).build();
assert_eq!(h.telemetry().value(GainTaps::LEVEL), None); // nothing published yet
h.inputs_mut().for_each(|lane| lane.fill(0.25));
h.process(&[]);
let level = h.telemetry().value(GainTaps::LEVEL).unwrap();
assert!((level - 0.25 * f64::from(db_to_amp(6.0))).abs() < 1e-6);
```

`Tap::set` is a few relaxed atomic operations on a slot resolved when
the taps were built: no lookup, no lock, no allocation on the audio thread.

## Step 2: Declare the face

[`Plugin::face`](stave::Plugin::face) is a pure function of the params
that returns a [`Face`](stave::Face): a layout tree of parts on a
[`Theme`](stave::face::Theme). Parts bind to params with `p.x.bind()`
and read taps by `TapId`; `row` and `column` lay them out; a
[`Plate`](stave::face::Plate) groups controls under a title, and a
[`Panel`](stave::face::Panel) is the enclosure. Parts never pick a
colour, a font or a coordinate: the theme holds the whole look, and
`theme.space(n)` is its spacing scale.

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
# #[derive(Taps)]
# pub struct GainTaps { pub level: Tap }
# pub struct Gain { params: Arc<GainParams>, gain: Smoother, taps: GainTaps }
# impl Plugin for Gain {
#     const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
#     type Params = GainParams;
#     fn new(params: &Arc<GainParams>, telemetry: &Telemetry) -> Self {
#         Gain { params: params.clone(), gain: Smoother::new(&params.gain), taps: GainTaps::new(telemetry) }
#     }
#     fn prepare(&mut self, env: &Environment) { self.gain.prepare(env); }
#     fn process(&mut self, block: &mut Block<'_>) {
#         let mut gain = self.gain.ramp(block, &self.params.gain);
#         let peaks = block.lanes::<2>().run(|frame| {
#             let g = db_to_amp(gain.next());
#             for lane in 0..frame.lanes() { frame.write(lane, frame.input(lane) * g); }
#         });
#         self.taps.level.set(f64::from(peaks.output));
#     }
    fn face(p: &GainParams) -> Option<Face> {
        let theme = Theme::graphite();
        let header = row(vec![
            Title::new("GAIN").tagline("±24 dB").node(),
            Footswitch::bypass(p.bypass.bind()).compact().node(),
        ])
        .gap(theme.space(4))
        .align(Align::Start);
        let level = Plate::titled("LEVEL").around(
            row(vec![
                Knob::new(p.gain.bind()).large().centered().node(),
                Meter::linear(GainTaps::LEVEL).label("OUT").height(90.0).node().id("meter"),
            ])
            .gap(theme.space(4))
            .align(Align::Center),
        );
        let body = column(vec![header, level]).gap(theme.space(3)).align(Align::Stretch);
        Some(Face::new(theme, Panel::default().around(body)))
    }
# }
#
# let params = GainParams::default();
# assert!(Gain::face(&params).is_some());
```

[`Meter::linear`](stave::face::Meter::linear) reads a linear amplitude
and shows it in dB; [`Meter::db`](stave::face::Meter::db) reads a tap
that is in dB already. `.centered()` marks the middle of the knob's
scale, the shape for a value whose rest is 0 dB.

## Step 3: Drive it like a person

[`FaceDriver::of`](stave_test::FaceDriver::of) builds the plugin's face
over a fresh instance of its params, with no window. Aim at a part by
name (its bound param's name, its role, or a `node.id`), then press,
drag, click, scroll and use the keys; read back values, the text the face
shows, and every edit it sent the host.

```
# use stave::prelude::*;
use stave_test::face::{dy, FaceDriver, Key};
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
# #[derive(Taps)]
# pub struct GainTaps { pub level: Tap }
# pub struct Gain { params: Arc<GainParams>, gain: Smoother, taps: GainTaps }
# impl Plugin for Gain {
#     const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
#     type Params = GainParams;
#     fn new(params: &Arc<GainParams>, telemetry: &Telemetry) -> Self {
#         Gain { params: params.clone(), gain: Smoother::new(&params.gain), taps: GainTaps::new(telemetry) }
#     }
#     fn prepare(&mut self, env: &Environment) { self.gain.prepare(env); }
#     fn process(&mut self, block: &mut Block<'_>) {
#         let mut gain = self.gain.ramp(block, &self.params.gain);
#         let peaks = block.lanes::<2>().run(|frame| {
#             let g = db_to_amp(gain.next());
#             for lane in 0..frame.lanes() { frame.write(lane, frame.input(lane) * g); }
#         });
#         self.taps.level.set(f64::from(peaks.output));
#     }
#     fn face(p: &GainParams) -> Option<Face> {
#         let theme = Theme::graphite();
#         let header = row(vec![
#             Title::new("GAIN").tagline("±24 dB").node(),
#             Footswitch::bypass(p.bypass.bind()).compact().node(),
#         ]).gap(theme.space(4)).align(Align::Start);
#         let level = Plate::titled("LEVEL").around(
#             row(vec![
#                 Knob::new(p.gain.bind()).large().centered().node(),
#                 Meter::linear(GainTaps::LEVEL).label("OUT").height(90.0).node().id("meter"),
#             ]).gap(theme.space(4)).align(Align::Center),
#         );
#         let body = column(vec![header, level]).gap(theme.space(3)).align(Align::Stretch);
#         Some(Face::new(theme, Panel::default().around(body)))
#     }
# }
let mut ui = FaceDriver::of::<Gain>();

// Drag the knob up a quarter of its travel: a quarter of 48 dB.
ui.press("gain").move_to(dy(-55.0));
assert_eq!(ui.value("gain"), 12.0);
assert!(ui.shows("12.0 dB"), "the readout shows while dragging: {:?}", ui.texts());
ui.release().exit();

// A double click returns it to its default.
ui.double_click("gain");
assert_eq!(ui.value("gain"), 0.0);

// The keyboard moves the focused knob.
ui.click("gain").key(Key::End);
assert_eq!(ui.value("gain"), 24.0);

// The bypass footswitch latches.
ui.click("bypass");
assert_eq!(ui.value("bypass"), 1.0);

// Every edit reached the host inside a begin … end gesture.
ui.assert_bracketed();

// The meter reads the plugin's tap and cannot be grabbed.
assert!(ui.runtime().taps().contains(&GainTaps::LEVEL));
assert!(ui.grabs("meter").is_empty());
```

Each edit a face makes is a host gesture: begin, one or more values,
end. `assert_bracketed` fails the test if any edit escaped its gesture
or a gesture was left open, which is what keeps automation recording
clean in every DAW.

## Next

These are the face's own promises. The laws every face keeps (every
bind resolves, every grab lands, gestures stay bracketed in random
sessions on every page) are mounted for you by
`stave_test::plugin_checks!`; [Prove it](crate::tutorials::prove_it)
sets that up, and [Faces, parts and themes](crate::concepts::faces)
covers the design system in full.
