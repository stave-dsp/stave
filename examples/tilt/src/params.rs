//! Tilt's typed parameter table. Ids are FOREVER — pick once, never
//! reuse or renumber.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{BoolParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit};

#[derive(Params)]
pub struct TiltParams {
    /// The seesaw: negative = darker (lows up, highs down),
    /// positive = brighter. 0 is bit-flat (a law test).
    pub tilt: FloatParam,
    /// Output trim. Smoothing is DECLARED here (table-owned ramp).
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
}

impl TiltParams {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tilt: FloatParam::new(1, "tilt", 0.0, Range::new(-6.0, 6.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Tone")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
            output: FloatParam::new(2, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(3, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
        })
    }
}
