//! Stave Anvil. **Start here**: this file is the plugin's
//! root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! engine.rs   the ENGINE half — grows into engine/ components the
//!               day the second concept arrives (the growth rule)
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and YOUR LAWS (properties.rs — write the first one
//!               BEFORE the DSP; plugins/tilt tells you why)
//! ```
//!
//! STATUS: **R1 — RHYTHM**. Two complete rigs
//! crossfaded by FOCUS: one honest tube path, and the impossible
//! split where the chug cannot touch the teeth. Five laws green.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod controls;
mod editor;
mod engine;
mod params;
mod presets;

pub use engine::AnvilEngine;
#[doc(hidden)]
pub use engine::RcCoupledTriodeCascade;
pub use params::AnvilParameters;

impl Spec for AnvilEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.anvil.vst3",
        name: c"Stave Anvil",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"The impossible metalcore head.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<AnvilParameters> {
        AnvilParameters::new()
    }
    fn editor(params: &AnvilParameters) -> Option<stave_core::UiNode> {
        Some(editor::build_editor(params))
    }
    fn presets(params: &AnvilParameters) -> alloc::vec::Vec<stave_core::Preset> {
        presets::build_presets(params)
    }

    fn state_version() -> u32 {
        1
    }

    fn migrations() -> stave_core::state::Migrations {
        fn legacy_six_decibel_tone_value_to_position(value: f64) -> f64 {
            (value + 6.0) * (10.0 / 12.0)
        }

        fn legacy_midrange_tone_value_to_position(value: f64) -> f64 {
            (value + 8.0) * (10.0 / 12.0)
        }

        let mut migrations = stave_core::state::Migrations::default();
        migrations
            .rescaled_before(1, 6, legacy_six_decibel_tone_value_to_position)
            .rescaled_before(1, 7, legacy_midrange_tone_value_to_position)
            .rescaled_before(1, 8, legacy_six_decibel_tone_value_to_position);
        migrations
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Anvil = StavePlugin<AnvilEngine>;

stave_vst3_export!(
    AnvilEngine,
    component_cid: (0x5A908C88, 0x84EC0CD9, 0x1727EBCC, 0x38EB1EFE),
    controller_cid: (0xE2F5272C, 0x4B153E7C, 0xB777CA10, 0x96D452A6),
);

stave_clap::stave_clap_export!(AnvilEngine); // CLAP build, same cdylib

#[cfg(test)]
mod tests {
    use super::*;
    use stave_core::params::{AnyParam, FloatParam, Params, Range, Taper};
    use stave_core::state::{load, save, StateStore};

    #[derive(Params)]
    struct LegacyToneParameters {
        bass: FloatParam,
        midrange: FloatParam,
        treble: FloatParam,
    }

    fn legacy_tone_parameters() -> LegacyToneParameters {
        LegacyToneParameters {
            bass: FloatParam::new(6, "bass", 1.5, Range::new(-6.0, 6.0, Taper::Linear)),
            midrange: FloatParam::new(7, "mid", -2.5, Range::new(-8.0, 4.0, Taper::Linear)),
            treble: FloatParam::new(8, "treble", 3.0, Range::new(-6.0, 6.0, Taper::Linear)),
        }
    }

    #[test]
    fn legacy_tone_state_keeps_the_same_physical_knob_positions() {
        let legacy_parameters = legacy_tone_parameters();
        let legacy_state = save(&legacy_parameters, 0, &[], &StateStore::default());
        let current_parameters = AnvilParameters::new();
        load(
            &legacy_state,
            current_parameters.as_ref(),
            &<AnvilEngine as Spec>::migrations(),
            &mut StateStore::default(),
        )
        .unwrap();

        assert_eq!(current_parameters.bass_tone.plain(), 6.25);
        assert!((current_parameters.midrange_tone.plain() - 4.583_333_333_333_333).abs() < 1e-12);
        assert_eq!(current_parameters.treble_tone.plain(), 7.5);
    }

    #[test]
    fn current_tone_state_is_not_migrated_twice() {
        let source_parameters = AnvilParameters::new();
        (&source_parameters.bass_tone as &dyn AnyParam).set_plain(8.0);
        (&source_parameters.midrange_tone as &dyn AnyParam).set_plain(3.0);
        (&source_parameters.treble_tone as &dyn AnyParam).set_plain(6.0);
        let current_state = save(
            source_parameters.as_ref(),
            <AnvilEngine as Spec>::state_version(),
            &[],
            &StateStore::default(),
        );

        let loaded_parameters = AnvilParameters::new();
        load(
            &current_state,
            loaded_parameters.as_ref(),
            &<AnvilEngine as Spec>::migrations(),
            &mut StateStore::default(),
        )
        .unwrap();

        assert_eq!(loaded_parameters.bass_tone.plain(), 8.0);
        assert_eq!(loaded_parameters.midrange_tone.plain(), 3.0);
        assert_eq!(loaded_parameters.treble_tone.plain(), 6.0);
    }
}
