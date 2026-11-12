//! One modulation route as parameters: the seven a [`RouteSlot`] is
//! snapshotted from each block — source, target, depth, via, mode, lag,
//! quant — so automation, state, presets and validation cover
//! modulation with no extra machinery. The source and target vocabularies
//! are the instrument's (its `param_enum!`s); the shaping vocabulary
//! (mode, quant) is shared.

use stave_dsp::RouteSlot;

use crate::param_enum;
use crate::params::{EnumParam, FloatParam, ParamEnum, Params, Range, Smooth, Taper, Unit};

param_enum! {
    /// Route polarity: the source's natural ±1, or remapped 0..1
    /// (envelope-style opening).
    pub const ROUTE_MODES: RouteMode = ["BI", "UNI"];
}

param_enum! {
    /// Route quantizer: OFF, or snap the source to N steps (S&H character
    /// from any source; arpeggiated pitch from a saw).
    pub const ROUTE_QUANTS: RouteQuant = ["OFF", "2", "3", "4", "6", "8", "12", "16"];
}

/// The step counts `ROUTE_QUANTS` names (OFF = 0).
const QUANT_STEPS: [f32; 8] = [0.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0];

/// The most route slots a table can number ("src 16").
pub const MAX_ROUTES: usize = 16;

macro_rules! slot_names {
    ($field:literal) => {
        [
            concat!($field, " 1"),
            concat!($field, " 2"),
            concat!($field, " 3"),
            concat!($field, " 4"),
            concat!($field, " 5"),
            concat!($field, " 6"),
            concat!($field, " 7"),
            concat!($field, " 8"),
            concat!($field, " 9"),
            concat!($field, " 10"),
            concat!($field, " 11"),
            concat!($field, " 12"),
            concat!($field, " 13"),
            concat!($field, " 14"),
            concat!($field, " 15"),
            concat!($field, " 16"),
        ]
    };
}

#[derive(Params)]
pub struct RouteParams<Source: ParamEnum, Target: ParamEnum> {
    pub src: EnumParam<Source>,
    pub tgt: EnumParam<Target>,
    /// Bipolar depth.
    pub depth: FloatParam,
    /// A second source scaling the depth (mod-the-mod).
    pub via: EnumParam<Source>,
    pub mode: EnumParam<RouteMode>,
    /// One-pole slew on the shaped value.
    pub lag: FloatParam,
    pub quant: EnumParam<RouteQuant>,
}

impl<Source: ParamEnum, Target: ParamEnum> RouteParams<Source, Target> {
    /// Slot `slot` (1-based, as hosts show it) with ids `base_id + 1`
    /// through `base_id + 7` in field order — ids are forever, so the
    /// base is the plugin's to choose once.
    pub fn new(base_id: u32, slot: usize, group: &'static str) -> Self {
        assert!(
            (1..=MAX_ROUTES).contains(&slot),
            "route slot {slot} of 1..={MAX_ROUTES}"
        );
        let name = |names: [&'static str; MAX_ROUTES]| names[slot - 1];
        Self {
            src: EnumParam::new(base_id + 1, name(slot_names!("src")), Source::from_index(0))
                .with_group(group),
            tgt: EnumParam::new(base_id + 2, name(slot_names!("tgt")), Target::from_index(0))
                .with_group(group),
            depth: FloatParam::new(
                base_id + 3,
                name(slot_names!("depth")),
                0.0,
                Range::new(-1.0, 1.0, Taper::Linear),
            )
            .with_group(group)
            .with_smooth(Smooth::Off),
            via: EnumParam::new(base_id + 4, name(slot_names!("via")), Source::from_index(0))
                .with_group(group),
            mode: EnumParam::new(base_id + 5, name(slot_names!("mode")), RouteMode(0))
                .with_group(group),
            lag: FloatParam::new(
                base_id + 6,
                name(slot_names!("lag")),
                0.0,
                Range::new(0.0, 500.0, Taper::Linear),
            )
            .with_unit(Unit::Ms)
            .with_group(group)
            .with_smooth(Smooth::Off),
            quant: EnumParam::new(base_id + 7, name(slot_names!("quant")), RouteQuant(0))
                .with_group(group),
        }
    }

    /// This block's slot; the lag knob becomes a one-pole coefficient at
    /// the caller's block rate.
    pub fn slot(&self, block_seconds: f32) -> RouteSlot {
        let lag_ms = self.lag.value();
        RouteSlot {
            src: self.src.index(),
            tgt: self.tgt.index(),
            depth: self.depth.value(),
            via: self.via.index(),
            uni: self.mode.index() == 1,
            lag_k: if lag_ms <= 0.0 {
                1.0
            } else {
                1.0 - (-block_seconds / (lag_ms * 0.001)).exp()
            },
            quant: QUANT_STEPS[self.quant.index().min(QUANT_STEPS.len() - 1)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::AnyParam;

    param_enum! {
        const SOURCES: Source = ["OFF", "LFO", "VEL"];
    }
    param_enum! {
        const TARGETS: Target = ["PITCH", "LEVEL"];
    }

    #[test]
    fn a_slot_numbers_its_ids_and_names_from_its_base() {
        let routes: [RouteParams<Source, Target>; 2] =
            core::array::from_fn(|i| RouteParams::new(10 * (i as u32 + 1), i + 1, "Mod"));
        let ids: Vec<u32> = (0..routes.count())
            .map(|i| routes.by_index(i).unwrap().meta().id.0)
            .collect();
        assert_eq!(
            ids,
            [11, 12, 13, 14, 15, 16, 17, 21, 22, 23, 24, 25, 26, 27]
        );
        assert_eq!(routes.by_name("quant 2").unwrap().meta().id.0, 27);
    }

    #[test]
    fn the_slot_carries_the_knobs() {
        let r: RouteParams<Source, Target> = RouteParams::new(0, 1, "Mod");
        (&r.src as &dyn AnyParam).set_plain(2.0);
        (&r.depth as &dyn AnyParam).set_plain(-0.5);
        (&r.quant as &dyn AnyParam).set_plain(3.0);
        let slot = r.slot(0.001);
        assert_eq!(
            (slot.src, slot.depth, slot.quant, slot.lag_k),
            (2, -0.5, 4.0, 1.0)
        );
    }
}
