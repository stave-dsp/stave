//! Stave Chime — a polyphonic sine/saw synth. **Start here**: this
//! file is the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! engine/     the ENGINE half (split when the matrix arrived —
//!               the growth rule):
//!   mod.rs    ChimeVoice × 16 under a VoiceRack + the event bridge
//!   matrix.rs the modulation semantics: routes, declared ranges,
//!               per-voice evaluation
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and THE LAWS (pitch, velocity, release,
//!               steal, mid-block starts — tests/properties.rs)
//! ```
//!
//! STATUS: **the Voice form's first shipping consumer**
//! — sixteen voices, quietest-steal, ADSR,
//! sine or polyBLEP saw. Deliberately small: the product is the
//! FORM being provable in a real host.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;

pub use engine::ChimeEngine;
pub use params::ChimeParams;

impl Spec for ChimeEngine {
    /// An instrument: no audio in, stereo out — stated.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_SYNTH];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.chime.vst3",
        name: c"Stave Chime",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A polyphonic sine/saw synth.",
        features: &[
            PluginFeature::Instrument,
            PluginFeature::Synthesizer,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<ChimeParams> {
        ChimeParams::new()
    }
    fn editor(params: &ChimeParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Chime = StavePlugin<ChimeEngine>;

stave_vst3_export!(
    ChimeEngine,
    component_cid: (0x3E92B7C1, 0x4A5D4E88, 0xB1F0A6D2, 0x22810001),
    controller_cid: (0x3E92B7C1, 0x4A5D4E88, 0xB1F0A6D2, 0x22810002),
);

stave_clap::stave_clap_export!(ChimeEngine); // CLAP build, same cdylib
