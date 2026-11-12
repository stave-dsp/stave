//! Stave Dynamo — THE IMPOSSIBLE MACHINE: a
//! physical-modeling/evolving synth built from machinery that
//! never existed — rotor, wires with memory, one shared bridge,
//! one shared boiler, an escapement. **Start here**: this file is
//! the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! engine/     the ENGINE half (split when the matrix arrived —
//!               the growth rule):
//!   mod.rs    DynamoVoice × 16 under a VoiceRack + the event bridge
//!   matrix.rs the modulation semantics: routes, declared ranges,
//!               per-voice evaluation
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and THE LAWS (pitch, velocity, release,
//!               steal, mid-block starts — tests/properties.rs)
//! ```
//!
//! Sixteen voices under a quietest-steal rack, ADSR, the mod
//! matrix and the patch bay; each voice is a rotor driving a wire.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod machine;
mod params;

pub use engine::DynamoEngine;
pub use params::DynamoParams;

impl Spec for DynamoEngine {
    /// An instrument: no audio in, stereo out — stated.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_SYNTH];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.dynamo.vst3",
        name: c"Stave Dynamo",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"The impossible machine: rotor, wires with memory, shared bridge and boiler.",
        features: &[
            PluginFeature::Instrument,
            PluginFeature::Synthesizer,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<DynamoParams> {
        DynamoParams::new()
    }
    fn editor(params: &DynamoParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Dynamo = StavePlugin<DynamoEngine>;

stave_vst3_export!(
    DynamoEngine,
    component_cid: (0x8A29C64A, 0x4FC494A7, 0x0755565D, 0x7AFF1F4D),
    controller_cid: (0x7263AFDA, 0xD9CADF89, 0x25C02CF8, 0x7B51E498),
);

stave_clap::stave_clap_export!(DynamoEngine); // CLAP build, same cdylib
