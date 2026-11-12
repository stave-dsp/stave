# Parameters and smoothing

A plugin's parameters are one typed struct, and everything a host, a
face or a saved session knows about them flows from its declarations:
a forever id, a name, a range and taper, a unit that formats and parses,
and a declared glide. The plugin reads them in their natural types and
follows their changes with a [`Smoother`](stave::params::Smoother) or
[`Steps`](stave::params::Steps), never with a hand-rolled copy.

## The table

A params struct derives [`Params`](stave_macros::Params) and builds every field
in its `Default`. The derive makes the struct the plugin's table: hosts,
faces and state read it in declaration order and look parameters up by
id or by name.

```
use stave::prelude::*;

param_enum! {
    /// The filter's response.
    pub const MODES: Mode = ["Low pass", "Band pass", "High pass"];
}

#[derive(Params)]
pub struct FilterParams {
    pub cutoff: FloatParam,
    pub mode: EnumParam<Mode>,
    pub hold: BoolParam,
    pub bypass: BoolParam,
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams {
            cutoff: FloatParam::new(ParamId(1), "Cutoff", Range::log(20.0, 20_000.0))
                .default(1_000.0)
                .unit(Unit::Hz)
                .group("Filter"),
            mode: EnumParam::new(ParamId(2), "Mode", Mode(0)).group("Filter"),
            hold: BoolParam::new(ParamId(3), "Hold", false),
            bypass: BoolParam::bypass(ParamId(4), "Bypass"),
        }
    }
}

let p = FilterParams::default();
// Typed reads, already inside the declaration.
assert_eq!(p.cutoff.value(), 1_000.0_f32);
assert_eq!(p.mode.value(), Mode(0));
assert!(!p.hold.value());
// The table: declaration order, lookups by id and by name.
assert_eq!(p.count(), 4);
assert_eq!(p.by_id(ParamId(2)).unwrap().meta().name, "Mode");
assert_eq!(p.by_name("Cutoff").unwrap().meta().group, "Filter");
assert_eq!(Params::bypass(&p).unwrap().meta().id, ParamId(4));
assert!(p.validate().is_ok()); // ids unique, none reserved
```

The three kinds are:

- [`FloatParam`](stave::params::FloatParam): a continuous value in a
  [`Range`](stave::params::Range), read as `f32`.
- [`BoolParam`](stave::params::BoolParam): two states, read as `bool`,
  named `"off"`/`"on"` unless `.names(off, on)` says otherwise. Never
  smoothed: DSP that needs a click-free toggle crossfades on its own
  terms.
- [`EnumParam`](stave::params::EnumParam): a selector over a type that
  [`param_enum!`](stave::param_enum) declares together with its labels.
  The labels are the one list the face and the host both read.

**Ids are forever.** A [`ParamId`](stave::ParamId) is chosen once and
never reused or renumbered: hosts store automation and sessions against
it, and faces bind to it. Ids with the top bit set are reserved for the
framework, as is [`ParamId::PROGRAM`](stave::ParamId::PROGRAM), the
preset selector the wrappers add; [`Params::validate`](stave::params::Params::validate),
which the wrappers run when they create an instance, refuses them and
any duplicate.

## Ranges, defaults and units

Every `Range` constructor validates, so a range its taper cannot
represent never exists. Three name the tapers (the fourth,
`Range::new(min, max, taper)`, takes a [`Taper`](stave::params::Taper)
value):

- [`Range::linear(min, max)`](stave::params::Range::linear): a straight
  line, right for signed and symmetric ranges.
- [`Range::log(min, max)`](stave::params::Range::log): equal travel per
  octave, for frequencies and time constants; needs `0 < min`.
- [`Range::skewed(min, max, midpoint)`](stave::params::Range::skewed):
  `midpoint` sits at half travel, for ranges whose useful action
  clusters at one end.

```
use stave::prelude::*;

let freq = Range::log(20.0, 20_000.0);
assert!((freq.denormalize(0.5) - 632.455_5).abs() < 1e-3); // the geometric middle
let attack = Range::skewed(0.1, 100.0, 10.0);
assert!((attack.denormalize(0.5) - 10.0).abs() < 1e-9);
assert_eq!(freq.normalize(f64::NAN), 0.0); // NaN never reaches a host
```

```should_panic
use stave::prelude::*;
let _ = Range::log(0.0, 1.0); // a log range through zero is refused
```

A `FloatParam` starts at the value in its range nearest 0 unless
`.default(x)` says otherwise (clamped into the range), formats with two
decimals until `.unit(..)` names its unit, and glides with
[`Smooth::DEFAULT`](stave::params::Smooth::DEFAULT), a 15 ms exponential,
until `.smooth(..)` declares another:

```
use stave::prelude::*;

let trim = FloatParam::new(ParamId(1), "Trim", Range::linear(-12.0, 12.0));
assert_eq!(trim.value(), 0.0);
assert_eq!(trim.format(1.0).as_str(), "1.00");
assert_eq!(trim.smoothing(), Smooth::ms(15.0));

let floor = FloatParam::new(ParamId(2), "Floor", Range::linear(20.0, 80.0));
assert_eq!(floor.value(), 20.0); // nearest 0 inside the range

let late = FloatParam::new(ParamId(3), "Mix", Range::linear(0.0, 1.0)).default(2.0);
assert_eq!(late.value(), 1.0); // the default is clamped into the range
```

A [`Unit`](stave::params::Unit) formats a value for a host or a face and
parses typed entry back. Unit switches are judged on the value as it
would show, so text read back from a host parses to the value it shows:

```
use stave::prelude::*;

assert_eq!(Unit::Db.format(-3.0).as_str(), "-3.0 dB");
assert_eq!(Unit::DbFloor(-60).format(-60.0).as_str(), "-inf dB");
assert_eq!(Unit::Hz.format(440.0).as_str(), "440.0 Hz");
assert_eq!(Unit::Hz.format(2_400.0).as_str(), "2.40 kHz");
assert_eq!(Unit::Hz.format(999.96).as_str(), "1.00 kHz"); // rounds onto the switch
assert_eq!(Unit::Ms.format(1_250.0).as_str(), "1.25 s");
assert_eq!(Unit::Percent.format(0.5).as_str(), "50%");
assert_eq!(Unit::Ratio.format(4.0).as_str(), "4.0:1");
assert_eq!(Unit::Custom { label: "kΩ", decimals: 2 }.format(2.2).as_str(), "2.20 kΩ");

assert_eq!(Unit::Hz.parse("2.4k"), Some(2_400.0));
assert_eq!(Unit::Ms.parse("1.5 s"), Some(1_500.0));
assert_eq!(Unit::Percent.parse("50 %"), Some(0.5));
assert_eq!(Unit::DbFloor(-60).parse("-inf"), Some(-60.0));
assert_eq!(Unit::Db.format(f64::NAN).as_str(), "?");
```

A param's own [`format`](stave::params::AnyParam::format) uses its unit,
and its [`parse`](stave::params::AnyParam::parse) clamps into its range.

## Reading a value

`value()` reads any param in its natural type, already bounded by the
declaration: an `f32` for a `FloatParam`, a `bool`, or the enum. It is a
block-rate read: take it once per block and never restate the range at
the read site. The erased view, [`AnyParam`](stave::params::AnyParam),
is what wrappers, faces and state use: every value there is an `f64` in
the param's own units (a bool is 0 or 1, an enum its index), every write
clamps, and a NaN write lands on the default.

```
use stave::prelude::*;

let gain = FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0)).default(-6.0);
gain.set_value(100.0);
assert_eq!(gain.value(), 24.0); // clamped
gain.set_value(f64::NAN);
assert_eq!(gain.value(), -6.0); // NaN writes the default
gain.set_normalized(0.75);
assert_eq!(gain.value(), 12.0);
assert_eq!(gain.normalized(), 0.75);
```

## Smoothing: a glide per knob

A knob's target can jump; applying the jump directly clicks. A
[`Smoother`](stave::params::Smoother) is the plugin's per-frame glide
toward its param's target, running the shape the param declares:

- [`Smooth::ms(τ)`](stave::params::Smooth::ms): a one-pole exponential
  with time constant τ: the usual feel for gains and mixes.
- [`Smooth::linear_ms(t)`](stave::params::Smooth::linear_ms): a straight
  line that arrives in exactly `t`, for delay times and pitch.
- [`Smooth::Off`](stave::params::Smooth::Off): the target, immediately.

Build the smoother from its param in `Plugin::new`, `prepare` it in
`Plugin::prepare` (its coefficients derive from the sample rate there),
then take one [`Ramp`](stave::params::Ramp) per block with `ramp(block,
&param)` and call `next()` once per frame. The ramp honours the block's
timed changes, so host automation lands on its frame:

```
use stave::prelude::*;
use stave::process::ParamBlock;

let time = FloatParam::new(ParamId(1), "Time", Range::linear(0.0, 100.0))
    .smooth(Smooth::linear_ms(10.0));
let mut glide = Smoother::new(&time);
glide.prepare(&Environment::new(1_000.0, 64)); // 10 ms is 10 frames here

// The first block after prepare starts at the value.
assert_eq!(glide.ramp_over(ParamBlock::empty(), &time).next(), 0.0);

time.set_value(100.0); // a host moves it
let mut ramp = glide.ramp_over(ParamBlock::empty(), &time);
let frames: Vec<f32> = (0..12).map(|_| ramp.next()).collect();
assert_eq!(frames[0], 10.0);
assert_eq!(frames[4], 50.0);
assert_eq!(frames[9], 100.0); // arrives in exactly 10 frames
assert_eq!(frames[11], 100.0);
```

**Restores do not glide.** When a host loads a session or a preset,
every smoother reading a restored param jumps to the new value instead
of fading in. A param's value also jumps, rather than glides, on the
first block after a `prepare`.

```
use stave::prelude::*;
use stave::process::ParamBlock;

let gain = FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0));
let mut glide = Smoother::new(&gain);
glide.prepare(&Environment::new(48_000.0, 64));
let _ = glide.ramp_over(ParamBlock::empty(), &gain).next();

gain.restore(-12.0); // a session load
assert_eq!(glide.ramp_over(ParamBlock::empty(), &gain).next(), -12.0);
```

Several knobs that glide together can share one derive:
[`#[derive(Smoothers)]`](stave_macros::Smoothers) builds, prepares and ramps a
struct of smoothers named after the params they follow, and can hand
back each frame as a struct of plain values.

## Stepped params, at their frames

A bool or an enum does not glide; it changes at a frame.
[`Steps`](stave::params::Steps) follows one: `value()` is the value in
force, and `changes(block, &param)` yields each change of the block as
a [`Step`](stave::params::Step) at its exact frame:

```
use stave::prelude::*;
use stave::process::ParamBlock;
use stave_test::change;

let hold = BoolParam::new(ParamId(7), "Hold", false);
let mut steps = Steps::new(&hold);
assert!(!steps.value());

let points = [change(ParamId(7), 32, 1.0)];
hold.set_value(1.0); // the table already holds the block's last value
let seen: Vec<Step<bool>> = steps.changes_over(ParamBlock::new(&points), &hold).collect();
assert_eq!(seen, [Step { frame: 32, value: true }]);
assert!(steps.value());
```

## Watching for change

Some params do not feed a per-frame glide but rebuild structure: a
filter's coefficients, a curve. A [`Watch`](stave::params::Watch) polls a
fixed set once per block and reports whether any value differs from the
last poll. It compares values, not writes: a write of the same value is
not a change, and the first poll always reports one.

```
use stave::prelude::*;

let drive = FloatParam::new(ParamId(1), "Drive", Range::linear(0.0, 1.0));
let tone = FloatParam::new(ParamId(2), "Tone", Range::linear(0.0, 1.0));
let mut watch = Watch::<2>::new();
assert!(watch.changed([&drive, &tone]));  // the initial build
assert!(!watch.changed([&drive, &tone]));
tone.set_value(0.0);                       // the same value: no change
assert!(!watch.changed([&drive, &tone]));
tone.set_value(0.5);
assert!(watch.changed([&drive, &tone]));
watch.reset();                             // in prepare()
assert!(watch.changed([&drive, &tone]));
```

**Reset every watch in `prepare`.** A re-prepare wipes rate-derived
coefficients and any structure the params applied, neither of which the
params can see; the first poll after it must rebuild everything.

[`ModalWatch`](stave::params::ModalWatch) adds params that matter only
in one mode of a selector: every watch polls on every block, so a hidden
mode can never go stale, but a gated param fires only while its mode is
current. Put the selector itself in the base set, so entering a mode
rebuilds from the gated params' current values.

## Presets and migrations

Factory presets are [`Preset`](stave::Preset)s, values in each param's
own units keyed by id, returned by `Plugin::presets`; the wrappers expose
them as the host's program list.

Because ids are forever, a shipped param's range and taper are frozen
too: hosts store automation as normalized positions, so re-ranging an id
would remap every user's curves. Change a range with a new id, and map
old sessions forward with [`Migrations`](stave::state::Migrations) in
`Plugin::migrations`: `renamed(old, new)` loads an old id as a new one,
`rescaled(id, f)` rewrites a stored value (after renames, keyed by the
new id), and `rescaled_before(version, id, f)` rewrites it only in state
saved before `STATE_VERSION` reached `version`. Bump `STATE_VERSION`
whenever a migration must tell old state from new.

```
use stave::prelude::*;

let migrations = Migrations::default()
    .renamed(ParamId(4), ParamId(9))
    .rescaled_before(2, ParamId(9), |percent| percent / 100.0);
# let _ = migrations;
```

Saved state stores plain values keyed by id, never normalized ones, and
keeps what it does not understand: ids and sections from a newer
version survive a load and a save in an older one.
