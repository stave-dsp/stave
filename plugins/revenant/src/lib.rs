//! Stave Revenant — a listening machine. Inside is an echo-state
//! network (a reservoir computer, written from scratch) whose readout
//! learns, online, to predict what you will play a few milliseconds
//! from now. What you hear is its guess: a ghost a step ahead of you.
//! Let it dream and it hears itself instead of you — and keeps
//! playing, in the shape of what it learned, when you stop. Behind a
//! lab-instrument face: a porthole onto the living network, and a
//! SURPRISE meter. **Start here**: this file is the plugin's root;
//! everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  factory presets
//! editor.rs   the face: one photoreal scene
//! pedal/      the face's picture: the bake (render.rs) + the live
//!               network, meter and controls (mod.rs)
//! engine/     the ENGINE half:
//!   mod.rs        the spine: decimate → think → interpolate → mix
//!   brain.rs      Brain — the reservoir and its online readout
//!   telemetry.rs  Telemetry — surprise and neuron activity
//! tests/      the shared checks + THE PROPERTIES (properties.rs)
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;
mod pedal;
mod presets;

pub use engine::RevenantEngine;
pub use params::RevenantParams;

impl Spec for RevenantEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.revenant.vst3",
        name: c"Stave Revenant",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A neural network that learns your playing and keeps playing when you stop.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<RevenantParams> {
        RevenantParams::new()
    }
    fn editor(params: &RevenantParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &RevenantParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// How wrong its recent guesses were, 0 (calm) … 1 (lost).
pub const SURPRISE_TAP: &str = "surprise";

/// The neurons the face shows (the first 24 of the network).
pub const NEURON_TAPS: [&str; 24] = [
    "neuron 0", "neuron 1", "neuron 2", "neuron 3", "neuron 4", "neuron 5",
    "neuron 6", "neuron 7", "neuron 8", "neuron 9", "neuron 10", "neuron 11",
    "neuron 12", "neuron 13", "neuron 14", "neuron 15", "neuron 16", "neuron 17",
    "neuron 18", "neuron 19", "neuron 20", "neuron 21", "neuron 22", "neuron 23",
];

/// Everything the face reads: surprise, then the neurons.
pub const FACE_TAPS: [&str; 25] = [
    SURPRISE_TAP,
    NEURON_TAPS[0], NEURON_TAPS[1], NEURON_TAPS[2], NEURON_TAPS[3], NEURON_TAPS[4], NEURON_TAPS[5],
    NEURON_TAPS[6], NEURON_TAPS[7], NEURON_TAPS[8], NEURON_TAPS[9], NEURON_TAPS[10], NEURON_TAPS[11],
    NEURON_TAPS[12], NEURON_TAPS[13], NEURON_TAPS[14], NEURON_TAPS[15], NEURON_TAPS[16], NEURON_TAPS[17],
    NEURON_TAPS[18], NEURON_TAPS[19], NEURON_TAPS[20], NEURON_TAPS[21], NEURON_TAPS[22], NEURON_TAPS[23],
];

/// The plugin: engine + declarations under the framework shell.
pub type Revenant = StavePlugin<RevenantEngine>;

stave_vst3_export!(
    RevenantEngine,
    component_cid: (0x6774941E, 0x457E446D, 0x62BC0705, 0x061C8A4A),
    controller_cid: (0xCE935155, 0x1A4A48A1, 0x6524FDBE, 0xA4DFB0B5),
);

stave_clap::stave_clap_export!(RevenantEngine); // CLAP build, same cdylib
