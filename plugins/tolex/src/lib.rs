//! Stave Tolex. **Start here**: this file is the plugin's
//! root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id (the full
//!               planned ledger is in plugins/tolex/docs/design.md)
//! editor.rs   the UI tree, a pure function of params
//! presets.rs  factory presets — BREAKDOWN is the flagship
//! curve.rs    ONE truth for picture and sound: the full response
//!               assembled from the engine's display twins, plus
//!               the baked population envelope (statistics only)
//! scene.rs    the response display — live curve over the
//!               population territory (law-pinned: the picture
//!               cannot lie, tests/curve_properties.rs)
//! engine/     the ENGINE half — the cabinet in components:
//!   mod.rs      the orchestration (bind → render)
//!   motor.rs    the lumped electro-mechanical driver core (leaf)
//!   cabinet.rs  the open back's wrap-around rear path
//!   voice.rs    the modal fingerprint + position system
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and THE LAWS (properties.rs — the physics promises)
//! ```
//!
//! The motor (T/S + Bl(x)/Cms(x) + source impedance + box) feeds the
//! cone's modal fingerprint: fixed mode set with ring-law Qs,
//! mic-position weights + comb + macro walk, drive-softened breakup.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

pub mod curve;
mod editor;
mod engine;
mod params;
mod presets;
mod scene;

pub use engine::TolexEngine;
pub use params::TolexParams;

impl Spec for TolexEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.tolex.vst3",
        name: c"Stave Tolex",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A guitar cabinet, built not sampled.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<TolexParams> {
        TolexParams::new()
    }
    fn editor(params: &TolexParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &TolexParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Tolex = StavePlugin<TolexEngine>;

stave_vst3_export!(
    TolexEngine,
    component_cid: (0xEB796B35, 0xEDF8C2C4, 0xF9C39DCA, 0xBD83B4C7),
    controller_cid: (0x5E690385, 0xBCCBE4AB, 0x9D21E046, 0x541F0AC3),
);

stave_clap::stave_clap_export!(TolexEngine); // CLAP build, same cdylib
