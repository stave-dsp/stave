//! Stave Eddy — a delay/chorus. **Start here**: this file is the
//! plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! presets.rs  factory presets, pure function of params
//! engine/     the ENGINE half — components, one per concept:
//!   mod.rs    the spine: the struct + process, three named phases
//!   echo.rs   Echo — the M/S delay lanes + the feedback law
//!   motion.rs Motion — the LFO × recipe (pristine/Juno/Vibe over
//!               the kernel basis: shape ∘ lag ∘ drift)
//!   telemetry.rs Telemetry — the wave cycle + phase cursor out
//! tests/      the shared checks: validation, editor bindings, no-alloc
//!               (this project's OWN laws arrive with its DSP —
//!               a project without them is not done)
//! ```
//!
//! STATUS: a delay/chorus. Time (tape-bend glide), feedback
//! (capped below runaway), mix (bit-dry at zero, by law), and
//! MOTION: rate/depth plus a character menu whose vintage names are
//! RECIPES over kernel primitives, with the live wave on screen.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;
mod presets;

pub use engine::EddyEngine;
pub use params::EddyParams;

impl Spec for EddyEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.eddy.vst3",
        name: c"Stave Eddy",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A feedback delay whose read head breathes.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Delay,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<EddyParams> {
        EddyParams::new()
    }
    fn editor(params: &EddyParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &EddyParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Eddy = StavePlugin<EddyEngine>;

stave_vst3_export!(
    EddyEngine,
    component_cid: (0x5A7FEDD9, 0x6E5E4F88, 0x9D6D7E11, 0x55E90001),
    controller_cid: (0x5A7FEDD9, 0x6E5E4F88, 0x9D6D7E11, 0x55E90002),
);

stave_clap::stave_clap_export!(EddyEngine); // CLAP build, same cdylib
