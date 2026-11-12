//! Multiband's parameter table.
//!
//! Ids are forever. Bands interleave in the original declaration order,
//! so `BandParams::new` derives each band's ids from per-field bases.
//! Retired ids, never reused: 3 (lm/hm), 4 (attack), 5 (release),
//! 48 (input gain), 49 (output gain), 51 (mix) — none reached the
//! signal; sessions that carry them keep them as unknown state.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

use crate::saturation::SaturationCharacter;
use crate::style::Style;

pub const BANDS: usize = 3;

/// Each band's gain-reduction telemetry, in dB (≤ 0).
pub const GR_PROBES: [&str; BANDS] = ["gr_lo", "gr_mid", "gr_hi"];

/// The controls' glide: 20 ms, slow enough that threshold and crossover
/// sweeps never zipper.
const GLIDE: Smooth = Smooth::Exp { ms: 20.0 };

const DB: Unit = Unit::Db { floor_db: None };

macro_rules! band_names {
    ($field:literal) => {
        [
            concat!("lo ", $field),
            concat!("mid ", $field),
            concat!("hi ", $field),
        ]
    };
}

const GROUPS: [&str; BANDS] = ["Low Band", "Mid Band", "High Band"];

#[derive(Params)]
pub struct BandParams {
    pub threshold: FloatParam,
    pub ratio: FloatParam,
    pub knee: FloatParam,
    pub makeup: FloatParam,
    pub attack: FloatParam,
    pub release: FloatParam,
    pub style: EnumParam<Style>,
    /// Transient catch: shortens attack on jumps; 0 leaves it to the style.
    pub transient: FloatParam,
    /// Peak → RMS blend of the detector input.
    pub rms: FloatParam,
    /// Detector high-pass; below 10 Hz it is off.
    pub sidechain_hpf: FloatParam,
    pub drive: FloatParam,
    pub saturation: EnumParam<SaturationCharacter>,
    pub saturation_mix: FloatParam,
    /// Compressed-band against dry-band blend.
    pub mix: FloatParam,
    pub solo: BoolParam,
    pub mute: BoolParam,
}

impl BandParams {
    fn new(band: usize) -> Self {
        let id = |base: u32, stride: u32| base + stride * band as u32;
        let group = GROUPS[band];
        let float = |id: u32, names: [&'static str; BANDS], default, range, unit| {
            FloatParam::new(id, names[band], default, range)
                .with_group(group)
                .with_unit(unit)
                .with_smooth(GLIDE)
        };
        let unit_range = Range::new(0.0, 1.0, Taper::Linear);
        let plain = Unit::Plain { decimals: 2 };
        Self {
            threshold: float(
                id(6, 3),
                band_names!("threshold"),
                -18.0,
                Range::new(-60.0, 0.0, Taper::Linear),
                DB,
            ),
            ratio: float(
                id(7, 3),
                band_names!("ratio"),
                3.0,
                Range::new(1.0, 20.0, Taper::Log),
                Unit::Ratio,
            ),
            makeup: float(
                id(8, 3),
                band_names!("makeup"),
                0.0,
                Range::new(-12.0, 24.0, Taper::Linear),
                DB,
            ),
            drive: float(
                id(15, 1),
                band_names!("drive"),
                0.0,
                Range::new(0.0, 20.0, Taper::Linear),
                DB,
            ),
            saturation_mix: float(id(18, 1), band_names!("sat mix"), 1.0, unit_range, plain),
            style: EnumParam::new(id(21, 1), band_names!("style")[band], Style::Vca)
                .with_group(group),
            saturation: EnumParam::new(
                id(24, 1),
                band_names!("sat type")[band],
                SaturationCharacter::Tube,
            )
            .with_group(group),
            attack: float(
                id(27, 1),
                band_names!("attack"),
                8.0,
                Range::new(0.5, 200.0, Taper::Log),
                Unit::Ms,
            ),
            release: float(
                id(30, 1),
                band_names!("release"),
                100.0,
                Range::new(5.0, 1000.0, Taper::Log),
                Unit::Ms,
            ),
            mix: float(id(33, 1), band_names!("mix"), 1.0, unit_range, plain),
            sidechain_hpf: float(
                id(36, 1),
                band_names!("sc HPF"),
                0.0,
                Range::new(0.0, 500.0, Taper::Linear),
                Unit::Hz,
            ),
            rms: float(id(39, 1), band_names!("rms mix"), 0.0, unit_range, plain),
            knee: float(
                id(42, 1),
                band_names!("knee"),
                0.0,
                Range::new(0.0, 20.0, Taper::Linear),
                DB,
            ),
            transient: float(id(45, 1), band_names!("trans"), 0.0, unit_range, plain),
            solo: BoolParam::new(id(52, 2), band_names!("solo")[band], false).with_group(group),
            mute: BoolParam::new(id(53, 2), band_names!("mute")[band], false).with_group(group),
        }
    }
}

#[derive(Params)]
pub struct MbParams {
    pub xover_low: FloatParam,
    pub xover_high: FloatParam,
    #[param(group)]
    pub bands: [BandParams; BANDS],
    /// The framework gate owns click-free, latency-matched bypass.
    pub bypass: BoolParam,
    /// Detect from the sidechain input, when the host connects one.
    pub external_sidechain: BoolParam,
}

impl MbParams {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            xover_low: FloatParam::new(1, "low/mid", 250.0, Range::new(60.0, 1000.0, Taper::Log))
                .with_group("Crossover")
                .with_unit(Unit::Hz)
                .with_smooth(GLIDE),
            xover_high: FloatParam::new(
                2,
                "mid/high",
                2500.0,
                Range::new(1000.0, 10000.0, Taper::Log),
            )
            .with_group("Crossover")
            .with_unit(Unit::Hz)
            .with_smooth(GLIDE),
            bands: core::array::from_fn(BandParams::new),
            bypass: BoolParam::new(50, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            external_sidechain: BoolParam::new(58, "ext sc", false).with_group("Sidechain"),
        })
    }
}
