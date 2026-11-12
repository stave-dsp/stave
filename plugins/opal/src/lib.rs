//! Stave Opal — a 4-operator FM synth. **Start here**: this file is
//! the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id (per-op ids
//!               blocked in tens, so the table reads like the panel)
//! diagram.rs  the algorithm picture (opal's OWN, via the open
//!               scene kind — bespoke visuals live in the project)
//! editor.rs   the UI tree: global card + tabbed operator pages
//! presets.rs  factory presets, pure function of params
//! engine/     the ENGINE half:
//!   mod.rs    OpalVoice × 16 under a VoiceRack + the event bridge
//!   operators.rs  the FM core LEAF: wiring-as-data algorithms,
//!               phase-mod math, the op-4 feedback loop
//! tests/      the shared checks + THE FM PROPERTIES (sidebands land where
//!               Chowning says; algorithms are audibly distinct;
//!               feedback grows teeth; velocity tracks brightness)
//! ```
//!
//! STATUS: **tier 2 of the instrument arc** — the
//! Operator-class consumer: per-voice control instancing at full
//! scale (4 envelopes × 16 voices), algorithms as data. Remaining
//! tier-2 legs (recorded, not lost): pitch-bend/mod-wheel plumbing,
//! keytrack cells, the interactive envelope-editor widget.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod diagram;
mod editor;
mod engine;
mod params;
mod presets;

pub use engine::OpalEngine;
pub use params::OpalParams;

impl Spec for OpalEngine {
    /// An instrument: no audio in, stereo out — stated.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_SYNTH];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.opal.vst3",
        name: c"Stave Opal",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"A 4-operator FM synth: eight algorithms, velocity into brightness.",
        features: &[
            PluginFeature::Instrument,
            PluginFeature::Synthesizer,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<OpalParams> {
        OpalParams::new()
    }
    fn editor(params: &OpalParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &OpalParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Opal = StavePlugin<OpalEngine>;

stave_vst3_export!(
    OpalEngine,
    component_cid: (0x6B47D2A9, 0x91E34C77, 0x8AF5B0E1, 0x33920001),
    controller_cid: (0x6B47D2A9, 0x91E34C77, 0x8AF5B0E1, 0x33920002),
);

stave_clap::stave_clap_export!(OpalEngine); // CLAP build, same cdylib
