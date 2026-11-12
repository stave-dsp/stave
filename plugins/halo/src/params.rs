//! Halo's stable host parameter table.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{BoolParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit};

pub const DEFAULT_PREAMP_GAIN_DECIBELS: f64 = 6.0;

#[derive(Params)]
pub struct HaloParameters {
    pub preamp_gain: FloatParam,
    pub bass_tone: FloatParam,
    pub midrange_tone: FloatParam,
    pub treble_tone: FloatParam,
    pub headroom: FloatParam,
    pub bloom: FloatParam,
    pub reserve: FloatParam,
    pub response: FloatParam,
    pub output_level: FloatParam,
    pub bypass: BoolParam,
}

impl HaloParameters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            output_level: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            preamp_gain: FloatParam::new(
                3,
                "gain",
                DEFAULT_PREAMP_GAIN_DECIBELS,
                Range::new(0.0, 18.0, Taper::Linear),
            )
            .with_unit(Unit::Db { floor_db: None })
            .with_group("Preamp")
            .with_smooth(Smooth::Exp { ms: 8.0 }),
            bass_tone: FloatParam::new(4, "bass", 5.5, Range::new(0.0, 10.0, Taper::Linear))
                .with_unit(Unit::Plain { decimals: 1 })
                .with_group("Tone"),
            midrange_tone: FloatParam::new(5, "mid", 6.0, Range::new(0.0, 10.0, Taper::Linear))
                .with_unit(Unit::Plain { decimals: 1 })
                .with_group("Tone"),
            treble_tone: FloatParam::new(6, "treble", 5.5, Range::new(0.0, 10.0, Taper::Linear))
                .with_unit(Unit::Plain { decimals: 1 })
                .with_group("Tone"),
            headroom: FloatParam::new(7, "headroom", 0.7, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Dynamics"),
            bloom: FloatParam::new(8, "bloom", 0.45, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Dynamics"),
            reserve: FloatParam::new(9, "reserve", 0.7, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Dynamics"),
            response: FloatParam::new(10, "response", 0.6, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Dynamics"),
        })
    }
}
