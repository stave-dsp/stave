//! Aeolian's typed parameter table — the face, in the harp's own
//! words. Ids are FOREVER — pick once, never reuse or renumber.
//!
//! ```text
//!   ( WIND )   ┃┃┃┃┃┃┃┃┃┃┃┃   ( SUSTAIN )     ← the harp window:
//!              C C# D … A# B                     twelve live strings
//!   tone  drift  REGISTER  ◉air ◉sway  mix
//!   aeolian                         ✦ [foot]
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// The strings' register, named by its lowest string: C2 puts the
    /// twelve strings in octave 2 (C2 … B2), each step lifts them an
    /// octave, C6 is octave 6.
    pub const REGISTERS: Register = ["C2", "C3", "C4", "C5", "C6"];
}

#[derive(Params)]
pub struct AeolianParams {
    /// Output trim (host-side; not on the face).
    pub output: FloatParam,
    /// The footswitch (inverted: the jewel lamp lights when engaged).
    /// The framework gate owns click-free bypass.
    pub bypass: BoolParam,
    /// Clean dry ↔ the strings (bit-exact dry at 0).
    pub mix: FloatParam,
    /// Dark ↔ bright: the strings' damping and the wet's top end.
    pub tone: FloatParam,
    /// Depth of the motion: AIR's spread and SWAY's drift.
    pub drift: FloatParam,
    /// WIND: how easily a played note wakes its string — velocity. Near the
    /// top (with SUSTAIN near the top too) the noise floor alone is
    /// enough: the strings self-oscillate.
    pub wind: FloatParam,
    /// SUSTAIN: how long a woken string rings — the envelope's decay.
    pub sustain: FloatParam,
    /// Which octave the strings are tuned in (the chicken-head selector).
    pub register: EnumParam<Register>,
    /// AIR: an ensemble of three drifting copies — width.
    pub air: BoolParam,
    /// SWAY: each string's pitch drifts on its own.
    pub sway: BoolParam,
}

impl AeolianParams {
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
            mix: FloatParam::new(3, "mix", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Strings")
                .with_smooth(Smooth::Exp { ms: 10.0 }),
            tone: FloatParam::new(4, "tone", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Strings")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            drift: FloatParam::new(5, "drift", 0.3, unit())
                .with_unit(Unit::Percent)
                .with_group("Motion")
                .with_smooth(Smooth::Exp { ms: 40.0 }),
            wind: FloatParam::new(6, "wind", 0.5, unit())
                .with_unit(Unit::Percent)
                .with_group("Strings")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            sustain: FloatParam::new(7, "sustain", 0.6, unit())
                .with_unit(Unit::Percent)
                .with_group("Strings")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            register: EnumParam::new(8, "register", Register(2)).with_group("Strings"),
            air: BoolParam::new(9, "air", true)
                .with_group("Motion")
                .with_names("OFF", "ON"),
            sway: BoolParam::new(10, "sway", false)
                .with_group("Motion")
                .with_names("OFF", "ON"),
        })
    }
}
