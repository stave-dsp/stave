//! The storybook's demo param table — a representative surface for
//! every param-bound component: banded gains, a log frequency, a
//! mix, toggles in both roles, a power, selectors for both pager
//! kinds, and an XY pad pair. Ids are stable but NOTHING saves them:
//! the storybook has no sessions.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// The selector story's steps.
    pub const MODES: Mode = ["CLEAN", "WARM", "PUSHED"];
}

param_enum! {
    /// The param-Tabs story's pages (a SOUND switch, so it is a
    /// param — the ViewTabs demo beside it is the chrome kind).
    pub const PANES: Pane = ["ALPHA", "BETA", "GAMMA"];
}

#[derive(Params)]
pub struct StoryParams {
    pub input_gain: FloatParam,
    pub output_gain: FloatParam,
    pub gain_a: FloatParam,
    pub gain_b: FloatParam,
    pub gain_c: FloatParam,
    pub freq: FloatParam,
    pub mix: FloatParam,
    pub power: BoolParam,
    pub solo: BoolParam,
    pub mute: BoolParam,
    pub mode: EnumParam<Mode>,
    pub pane: EnumParam<Pane>,
    pub pad_x: FloatParam,
    pub pad_y: FloatParam,
    pub attack: FloatParam,
    pub decay: FloatParam,
    pub sustain: FloatParam,
    pub release: FloatParam,
}

impl StoryParams {
    pub fn new() -> Arc<Self> {
        let gain = |id: u32, name: &'static str, v: f64| {
            FloatParam::new(id, name, v, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Gains")
                .with_smooth(Smooth::Exp { ms: 5.0 })
        };
        Arc::new(Self {
            input_gain: gain(17, "input", 0.0).with_group("Shell"),
            output_gain: gain(18, "output", 0.0).with_group("Shell"),
            gain_a: gain(1, "gain a", 3.0),
            gain_b: gain(2, "gain b", -6.0),
            gain_c: gain(3, "gain c", 12.0),
            freq: FloatParam::new(4, "freq", 1_000.0, Range::new(20.0, 20_000.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Tone"),
            mix: FloatParam::new(5, "mix", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Tone"),
            power: BoolParam::new(6, "power", true)
                .with_group("Switches")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            solo: BoolParam::new(7, "solo", false).with_group("Switches"),
            mute: BoolParam::new(8, "mute", false).with_group("Switches"),
            mode: EnumParam::new(9, "mode", Mode(0)).with_group("Selectors"),
            pane: EnumParam::new(10, "pane", Pane(0)).with_group("Selectors"),
            pad_x: FloatParam::new(11, "bias", 0.0, Range::new(-1.0, 1.0, Taper::Linear))
                .with_group("Pad"),
            pad_y: FloatParam::new(12, "drive", 0.4, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Pad"),
            attack: FloatParam::new(13, "attack", 8.0, Range::new(0.5, 2_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope"),
            decay: FloatParam::new(14, "decay", 250.0, Range::new(1.0, 4_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope"),
            sustain: FloatParam::new(15, "sustain", 0.55, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Envelope"),
            release: FloatParam::new(16, "release", 400.0, Range::new(2.0, 8_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Envelope"),
        })
    }
}
