//! Chime's typed parameter table. Ids are FOREVER — pick once,
//! never reuse or renumber.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, RouteParams, Smooth, Taper, Unit,
};

param_enum! {
    /// The oscillator's shape — pure sine, or the polyBLEP saw.
    pub const SHAPES: Shape = ["SINE", "SAW"];
}

param_enum! {
    /// Modulation sources. OFF disables the
    /// slot. Per-voice: VEL, KEY, ENV (the voice's own envelope),
    /// SPREAD (a stable per-note random — unison detune generalized
    /// to any target). Global: LFO, HEAT (a slow fatigue integrator
    /// of playing intensity — components warming up), SYMP (the
    /// OTHER voices' energy bleeding in, sympathetic-string style).
    pub const SOURCES: ModSource =
        ["OFF", "LFO", "VEL", "KEY", "ENV", "SPREAD", "HEAT", "SYMP"];
}

param_enum! {
    /// The LFO's waveform. SINE..SQR are the dsp shape basis; S&H
    /// and DRIFT are RandomWalk recipes (stepped renewal / eased walk).
    pub const LFO_SHAPES: LfoShapeP = ["SINE", "TRI", "SAW", "SQR", "S&H", "DRIFT"];
}

param_enum! {
    /// Rate mode: free Hz from the knob, or tempo-locked divisions.
    pub const LFO_SYNC: LfoSync =
        ["FREE", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8T"];
}

param_enum! {
    /// Phase mode: FREE runs one global phase (chords shimmer);
    /// NOTE restarts each voice's phase at note-on (attacks agree).
    pub const LFO_TRIG: LfoTrig = ["FREE", "NOTE"];
}

param_enum! {
    /// Modulation destinations — the DECLARED allowlist (never "any
    /// param"): pitch in ±12 st, level in ±12 dB, decay stretched
    /// up to 4× either way.
    pub const TARGETS: ModTarget = ["PITCH", "LEVEL", "DECAY"];
}

// Default voicing is the NAMESAKE: a struck bell — zero sustain
// (the note fades even while held; decay is the body), long release
// (letting go lets it ring out). Organ/pad behavior is one knob away
// (sustain up).
#[derive(Params)]
pub struct ChimeParams {
    /// Attack: ARRIVAL time to full level (the Adsr contract).
    pub attack: FloatParam,
    /// Decay toward sustain (time constant).
    pub decay: FloatParam,
    /// Where a held note settles.
    pub sustain: FloatParam,
    /// The ring after note-off (time constant).
    pub release: FloatParam,
    /// The oscillator shape.
    pub shape: EnumParam<Shape>,
    /// The global LFO's rate (the matrix's clock).
    pub lfo_rate: FloatParam,
    pub lfo_shape: EnumParam<LfoShapeP>,
    pub lfo_sync: EnumParam<LfoSync>,
    /// Onset ramp: the LFO fades in over this many ms after
    /// note-on (a player's vibrato, not a machine's).
    pub lfo_rise: FloatParam,
    pub lfo_trig: EnumParam<LfoTrig>,
    /// The modulation routes: seven ordinary params per slot (see
    /// `RouteParams`), so automation, state and presets cover them.
    #[param(group)]
    pub routes: [RouteParams<ModSource, ModTarget>; 4],
    /// Output trim. Smoothing is DECLARED here (table-owned ramp).
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
}

impl ChimeParams {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            attack: FloatParam::new(1, "attack", 5.0, Range::new(0.5, 2_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope")
                .with_smooth(Smooth::Off),
            decay: FloatParam::new(2, "decay", 800.0, Range::new(1.0, 4_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope")
                .with_smooth(Smooth::Off),
            sustain: FloatParam::new(3, "sustain", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Envelope")
                .with_smooth(Smooth::Off),
            release: FloatParam::new(4, "release", 1_200.0, Range::new(2.0, 8_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope")
                .with_smooth(Smooth::Off),
            shape: EnumParam::new(5, "shape", Shape(0)).with_group("Voice"),
            lfo_rate: FloatParam::new(8, "lfo rate", 1.2, Range::new(0.05, 12.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Mod")
                .with_smooth(Smooth::Off),
            lfo_shape: EnumParam::new(9, "lfo shape", LfoShapeP(0)).with_group("Mod"),
            lfo_sync: EnumParam::new(10, "lfo sync", LfoSync(0)).with_group("Mod"),
            lfo_rise: FloatParam::new(18, "lfo rise", 0.0, Range::new(0.0, 2_000.0, Taper::Linear))
                .with_unit(Unit::Ms)
                .with_group("Mod")
                .with_smooth(Smooth::Off),
            lfo_trig: EnumParam::new(19, "lfo trig", LfoTrig(0)).with_group("Mod"),
            routes: core::array::from_fn(|i| RouteParams::new(10 * (i as u32 + 1), i + 1, "Mod")),
            output: FloatParam::new(6, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(7, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
        })
    }
}
