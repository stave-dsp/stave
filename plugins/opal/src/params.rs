//! Opal's typed parameter table. Ids are FOREVER — pick once, never
//! reuse or renumber. Four operators × (ratio, level, ADSR) + the
//! global surface. Per-operator ids are blocked in tens (op1 = 10s,
//! op2 = 20s, …) so the table reads like the panel.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, RouteParams, Smooth, Taper, Unit,
};

param_enum! {
    /// The eight algorithms — mod wiring as data (engine/operators.rs
    /// owns the actual tables; these labels are the selector truth).
    /// CHAIN 4→3→2→1; FORK (3,4)→2→1; TWIN (2, 3←4)→1;
    /// STACK2 two 2-op pairs; TRIPLE 4→(1,2,3); BRIGHT (2,3,4)→1;
    /// ORGAN2 2→1 + two pure; ORGAN additive.
    pub const ALGOS: Algo = [
        "CHAIN", "FORK", "TWIN", "STACK2", "TRIPLE", "BRIGHT", "ORGAN2", "ORGAN",
    ];
}

/// The operator page labels — pure editor chrome (the pager is a
/// ViewTabs over a view key: no param, no automation, no state).
pub const OPS: &[&str] = &["OP 1", "OP 2", "OP 3", "OP 4"];

param_enum! {
    /// Modulation sources (the chime vocabulary). OFF
    /// disables. Per-voice: VEL, KEY, ENV (the carrier envelope),
    /// SPREAD (stable per-note random). Global: LFO, HEAT (fatigue
    /// integrator), SYMP (the other voices' energy).
    pub const SOURCES: ModSource =
        ["OFF", "LFO 1", "VEL", "KEY", "ENV", "SPREAD", "HEAT", "SYMP", "LFO 2"];
}

param_enum! {
    /// The LFO's waveform (dsp basis + RandomWalk recipes).
    pub const LFO_SHAPES: LfoShapeP = ["SINE", "TRI", "SAW", "SQR", "S&H", "DRIFT"];
}

param_enum! {
    /// Rate mode: free Hz, or tempo-locked divisions.
    pub const LFO_SYNC: LfoSync =
        ["FREE", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8T"];
}

param_enum! {
    /// FREE = one global phase; NOTE = per-voice restart at note-on.
    pub const LFO_TRIG: LfoTrig = ["FREE", "NOTE"];
}

param_enum! {
    /// Modulation destinations (APPEND-only — step indices are
    /// state) — the DECLARED allowlist. The
    /// modulator LEVELS are the FM-musical ones: LFO→LEVEL 2 is the
    /// classic wah, KEY→LEVEL 4 is keyboard-tracked brightness.
    pub const TARGETS: ModTarget = [
        "PITCH", "LEVEL 2", "LEVEL 3", "LEVEL 4", "FEEDBACK",
        // The Operator lesson: op VOLUMES and pitch are first-class
        // destinations; RATIO 2 is inharmonicity (FM chorus at
        // small depths); LFO2 RATE is Absynth's cross-LFO motion.
        "LEVEL 1", "RATIO 2", "LFO2 RATE",
    ];
}

#[derive(Params)]
pub struct OpalParams {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp).
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
    /// Which wiring runs (mod matrix as data).
    pub algo: EnumParam<Algo>,
    /// Op 4's self-feedback — a pure sine grows sawtooth teeth.
    pub feedback: FloatParam,
    /// Velocity → modulator depth: 0 = velocity only moves loudness,
    /// 1 = soft notes are pure and hard notes are bright (the FM
    /// expressiveness knob).
    /// (id 6 was `op`, the editor's page selector masquerading as a
    /// param — replaced by a ViewKey; the id is never reused.)
    pub vel_mod: FloatParam,
    /// The matrix clock.
    pub lfo_rate: FloatParam,
    pub lfo_shape: EnumParam<LfoShapeP>,
    pub lfo_sync: EnumParam<LfoSync>,
    pub lfo_rise: FloatParam,
    pub lfo_trig: EnumParam<LfoTrig>,
    /// The SECOND LFO (the Absynth lesson: independent motions
    /// compound; one LFO is a wobble, two are weather). Same voice
    /// of options as LFO 1, defaulting slower.
    pub lfo2_rate: FloatParam,
    pub lfo2_shape: EnumParam<LfoShapeP>,
    pub lfo2_sync: EnumParam<LfoSync>,
    pub lfo2_rise: FloatParam,
    pub lfo2_trig: EnumParam<LfoTrig>,
    /// The modulation routes: seven ordinary params per slot (see
    /// `RouteParams`), so automation, state and presets cover them.
    #[param(group)]
    pub routes: [RouteParams<ModSource, ModTarget>; 4],
    // ---- OP 1 (the anchor carrier) ------------------------------
    pub ratio1: FloatParam,
    pub level1: FloatParam,
    pub attack1: FloatParam,
    pub decay1: FloatParam,
    pub sustain1: FloatParam,
    pub release1: FloatParam,
    // ---- OP 2 ----------------------------------------------------
    pub ratio2: FloatParam,
    pub level2: FloatParam,
    pub attack2: FloatParam,
    pub decay2: FloatParam,
    pub sustain2: FloatParam,
    pub release2: FloatParam,
    // ---- OP 3 ----------------------------------------------------
    pub ratio3: FloatParam,
    pub level3: FloatParam,
    pub attack3: FloatParam,
    pub decay3: FloatParam,
    pub sustain3: FloatParam,
    pub release3: FloatParam,
    // ---- OP 4 (the top of every stack; owns the feedback) --------
    pub ratio4: FloatParam,
    pub level4: FloatParam,
    pub attack4: FloatParam,
    pub decay4: FloatParam,
    pub sustain4: FloatParam,
    pub release4: FloatParam,
}

fn ratio(id: u32, name: &'static str) -> FloatParam {
    FloatParam::new(id, name, 1.0, Range::new(0.25, 16.0, Taper::Log))
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

fn level(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(0.0, 1.0, Taper::Linear))
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

fn a(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(0.5, 2_000.0, Taper::Log))
        .with_unit(Unit::Ms)
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

fn d(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(1.0, 4_000.0, Taper::Log))
        .with_unit(Unit::Ms)
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

fn sus(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(0.0, 1.0, Taper::Linear))
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

fn r(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(id, name, default, Range::new(2.0, 8_000.0, Taper::Log))
        .with_unit(Unit::Ms)
        .with_group("Operators")
        .with_smooth(Smooth::Off)
}

impl OpalParams {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            output: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            algo: EnumParam::new(3, "algo", Algo(0)).with_group("Global"),
            feedback: FloatParam::new(4, "feedback", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Global")
                .with_smooth(Smooth::Off),
            vel_mod: FloatParam::new(5, "vel mod", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Global")
                .with_smooth(Smooth::Off),
            lfo_shape: EnumParam::new(47, "lfo shape", LfoShapeP(0)).with_group("Mod"),
            lfo_sync: EnumParam::new(48, "lfo sync", LfoSync(0)).with_group("Mod"),
            lfo_rise: FloatParam::new(49, "lfo rise", 0.0, Range::new(0.0, 2_000.0, Taper::Linear))
                .with_unit(Unit::Ms)
                .with_group("Mod")
                .with_smooth(Smooth::Off),
            lfo_trig: EnumParam::new(50, "lfo trig", LfoTrig(0)).with_group("Mod"),
            lfo2_rate: FloatParam::new(88, "lfo2 rate", 0.3, Range::new(0.05, 12.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Mod")
                .with_smooth(Smooth::Off),
            lfo2_shape: EnumParam::new(89, "lfo2 shape", LfoShapeP(0)).with_group("Mod"),
            lfo2_sync: EnumParam::new(90, "lfo2 sync", LfoSync(0)).with_group("Mod"),
            lfo2_rise: FloatParam::new(
                91,
                "lfo2 rise",
                0.0,
                Range::new(0.0, 2_000.0, Taper::Linear),
            )
            .with_unit(Unit::Ms)
            .with_group("Mod")
            .with_smooth(Smooth::Off),
            lfo2_trig: EnumParam::new(92, "lfo2 trig", LfoTrig(0)).with_group("Mod"),
            lfo_rate: FloatParam::new(7, "lfo rate", 1.2, Range::new(0.05, 12.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Mod")
                .with_smooth(Smooth::Off),
            routes: core::array::from_fn(|i| RouteParams::new(50 + 10 * i as u32, i + 1, "Mod")),
            ratio1: ratio(10, "ratio 1"),
            level1: level(11, "level 1", 1.0),
            attack1: a(12, "attack 1", 2.0),
            decay1: d(13, "decay 1", 600.0),
            sustain1: sus(14, "sustain 1", 0.7),
            release1: r(15, "release 1", 300.0),

            ratio2: ratio(20, "ratio 2"),
            level2: level(21, "level 2", 0.5),
            attack2: a(22, "attack 2", 2.0),
            decay2: d(23, "decay 2", 400.0),
            sustain2: sus(24, "sustain 2", 0.5),
            release2: r(25, "release 2", 300.0),

            ratio3: ratio(30, "ratio 3"),
            level3: level(31, "level 3", 0.0),
            attack3: a(32, "attack 3", 2.0),
            decay3: d(33, "decay 3", 400.0),
            sustain3: sus(34, "sustain 3", 0.5),
            release3: r(35, "release 3", 300.0),

            ratio4: ratio(40, "ratio 4"),
            level4: level(41, "level 4", 0.0),
            attack4: a(42, "attack 4", 2.0),
            decay4: d(43, "decay 4", 400.0),
            sustain4: sus(44, "sustain 4", 0.5),
            release4: r(45, "release 4", 300.0),
        })
    }
}
