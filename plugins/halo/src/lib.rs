//! Stave Halo: a high-headroom clean amplifier whose attack can remain cleaner
//! than its sustain.
//!
//! Halo is a head, not a cabinet or spatial effect. Its audio path is singular;
//! spectral analysis exists only in the control plane, where independent power
//! reservoirs prevent bass energy from automatically consuming the chime
//! reservoir.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod controls;
mod editor;
mod engine;
mod params;
mod presets;

pub use engine::HaloEngine;
pub use params::HaloParameters;

impl Spec for HaloEngine {
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.halo.vst3",
        name: c"Stave Halo",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"The impossible high-headroom clean amplifier.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<HaloParameters> {
        HaloParameters::new()
    }

    fn editor(parameters: &HaloParameters) -> Option<stave_core::UiNode> {
        Some(editor::build_editor(parameters))
    }

    fn presets(parameters: &HaloParameters) -> alloc::vec::Vec<stave_core::Preset> {
        presets::build_presets(parameters)
    }
}

pub type Halo = StavePlugin<HaloEngine>;

stave_vst3_export!(
    HaloEngine,
    component_cid: (0xB5D1B293, 0x88B54BE7, 0xA96D3431, 0xD624F842),
    controller_cid: (0x49084B2F, 0xAD5D41A8, 0xBBC0A431, 0xB9378EE4),
);

stave_clap::stave_clap_export!(HaloEngine);
