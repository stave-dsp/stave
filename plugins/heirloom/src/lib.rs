//! Stave Heirloom — saga's reverse machines (reverse delays and
//! reverbs on a clock you can pull) played back off a worn record: a
//! Warped Vinyl-style platter wobble and a Generation Loss-style
//! wear stage, behind a six-knob pedal face. saga's docs are the
//! map. **Start here**: this file is the plugin's root; everything
//! below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  factory presets
//! editor.rs   the face: one photoreal pedal scene
//! pedal/      the pedal's picture — a software-lit enclosure bake
//!               (render.rs) + the live knobs/toggles/LEDs (face.rs)
//! engine/     the ENGINE half:
//!   mod.rs        the spine: bind → render → observe
//!   motion.rs     Motion — the core clock: base, HOLD, MOD LFO
//!   clock.rs      ClockDomain — host rate ⇄ the core clock
//!   programs.rs   Programs — the reverse machines
//!   reverse.rs    ReverseLine — a line read backward
//!   tank.rs       Tank — the 8-line FDN reverb
//!   transpose.rs  Transposer — the two-head pitch shifter
//!   path.rs       AnalogPath — the feedback loops' analog legs
//!   wet.rs        Wet — tone, width, the soft ceiling
//!   media.rs      Media — WOBBLE + AGE, the wet path's record
//!   telemetry.rs  Telemetry — clock + tail out
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

pub use engine::HeirloomEngine;
pub use params::HeirloomParams;

impl Spec for HeirloomEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.heirloom.vst3",
        name: c"Stave Heirloom",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"Reverse machines played back off a worn record.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Reverb,
            PluginFeature::Delay,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<HeirloomParams> {
        HeirloomParams::new()
    }
    fn editor(params: &HeirloomParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &HeirloomParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Heirloom = StavePlugin<HeirloomEngine>;

stave_vst3_export!(
    HeirloomEngine,
    component_cid: (0x68E5A59A, 0x63E215C2, 0x02E460FC, 0x637F31D7),
    controller_cid: (0x997883AB, 0xFACDA952, 0x33F55080, 0xD9A58D26),
);

stave_clap::stave_clap_export!(HeirloomEngine); // CLAP build, same cdylib
