//! Saga's typed parameter table. Ids are FOREVER — pick once,
//! never reuse or renumber. The table reads like the pedal it
//! answers (docs/saga.md): the eight face knobs, the MODE button,
//! the five TWEAK functions broken out as their own knobs, the
//! Tap/Shift footswitch's hold as DIVE, and host sync where the
//! pedal has tap tempo.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// The five programs — each pairs two machines in series, each
    /// with its own feedback path (FEEDBACK drives the first, REGEN
    /// the second):
    /// I   reverse delay → reverse reverb
    /// II  reverse delay → octave-up reverb
    /// III reverse delay → octave-down reverb
    /// IV  reverse reverb → forward reverb
    /// V   pitch delay → pitch delay
    pub const MODES: Mode = ["I", "II", "III", "IV", "V"];
}

param_enum! {
    /// The modulation LFO's shapes (TWEAK → MOD SHAPE).
    pub const MOD_SHAPES: ModShape = ["SINE", "SQUARE", "RANDOM"];
}

param_enum! {
    /// TIME's clock source. FREE = the knob; the divisions follow the
    /// host tempo (the plugin's answer to the pedal's tap switch).
    pub const SYNCS: Sync = ["FREE", "1/2", "1/4", "1/8.", "1/8", "1/8T", "1/16"];
}

#[derive(Params)]
pub struct SagaParams {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp).
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
    /// Which program runs.
    pub mode: EnumParam<Mode>,
    /// Gain of the FIRST feedback path — delay repeats (I–III, V), or
    /// octave-down + reverse-reverb feedback (IV).
    pub feedback: FloatParam,
    /// Gain of the SECOND feedback path — reverse-reverb feedback (I),
    /// octave repeats into the tank (II, III), octave-up + reverse
    /// feedback (IV), the second pitch delay's repeats (V).
    pub regen: FloatParam,
    /// Depth of the clock modulation — the whole wet machine wobbles.
    pub mod_depth: FloatParam,
    /// Wet/dry: fully left = dry (bit-exact, a property test), right = wet.
    pub mix: FloatParam,
    /// The mode-dependent knob: how reversed (I), reverb decay
    /// (II–IV), interval order of the pitch sequence (V).
    pub x: FloatParam,
    /// Delay time (I–III, V); the time-stretch of the space (IV).
    pub time: FloatParam,
    /// The wet path's synth-style low-pass: closed = lows only, open
    /// = the full band.
    pub tone: FloatParam,
    /// TWEAK: how far the wet image spreads between the speakers.
    pub width: FloatParam,
    /// TWEAK: the modulation LFO's rate.
    pub mod_rate: FloatParam,
    /// TWEAK: the modulation LFO's shape.
    pub mod_shape: EnumParam<ModShape>,
    /// TWEAK: where DIVE/RISE takes the clock — 0.5× dives an octave,
    /// 2× rises one.
    pub shift: FloatParam,
    /// TWEAK: how long the clock takes to get to SHIFT, and back.
    pub ramp: FloatParam,
    /// The Tap/Shift footswitch held down: the clock glides to SHIFT
    /// and stays there while this is on.
    pub dive: BoolParam,
    /// TIME follows the host tempo at this division (FREE = the knob).
    pub sync: EnumParam<Sync>,
}

impl SagaParams {
    pub fn new() -> Arc<Self> {
        let unit = || Range::new(0.0, 1.0, Taper::Linear);
        Arc::new(Self {
            output: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags { bypass: true, ..Default::default() }),
            mode: EnumParam::new(3, "mode", Mode(0)).with_group("Program"),
            feedback: FloatParam::new(4, "feedback", 0.45, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            regen: FloatParam::new(5, "regen", 0.4, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            mod_depth: FloatParam::new(6, "mod", 0.25, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            mix: FloatParam::new(7, "mix", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 10.0 }),
            x: FloatParam::new(8, "x", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 60.0 }),
            time: FloatParam::new(9, "time", 450.0, Range::new(50.0, 1200.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 150.0 }),
            tone: FloatParam::new(10, "tone", 0.75, unit())
                .with_unit(Unit::Percent)
                .with_group("Program")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            width: FloatParam::new(11, "width", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Tweak")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            mod_rate: FloatParam::new(12, "mod rate", 0.6, Range::new(0.1, 5.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Tweak")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            mod_shape: EnumParam::new(13, "mod shape", ModShape(0)).with_group("Tweak"),
            shift: FloatParam::new(14, "shift", 0.5, Range::new(0.5, 2.0, Taper::Log))
                .with_unit(Unit::Plain { decimals: 2 })
                .with_group("Tweak"),
            ramp: FloatParam::new(15, "ramp", 600.0, Range::new(50.0, 5000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Tweak"),
            dive: BoolParam::new(16, "dive", false)
                .with_group("Tweak")
                .with_names("OFF", "HELD"),
            sync: EnumParam::new(17, "sync", Sync(0)).with_group("Program"),
        })
    }
}
