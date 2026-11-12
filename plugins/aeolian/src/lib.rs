//! Stave Aeolian — a sympathetic string harp: twelve plucked-string
//! voices, one per pitch class, that ring when you play their note in
//! any octave. WIND is velocity, SUSTAIN the decay, REGISTER the
//! octave; AIR and SWAY move it. Behind a photoreal verdigris-and-
//! brass face whose harp window shows the twelve strings themselves,
//! vibrating as they sing. **Start here**: this file is the plugin's
//! root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  factory presets
//! editor.rs   the face: one photoreal pedal scene
//! pedal/      the pedal's picture: the bake (render.rs, on the shared
//!               stave_ui::photo studio) + the live parts (mod.rs)
//! engine/     the ENGINE half:
//!   mod.rs        the spine: bind → render → observe
//!   detect.rs     Detector — twelve pitch-class bands → excitation
//!   strings.rs    Strings — twelve doubled Karplus–Strong voices
//!   ensemble.rs   Ensemble — CHORUS
//!   telemetry.rs  Telemetry — each string's ring + the clock → the face
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

pub use engine::AeolianEngine;
pub use params::AeolianParams;

impl Spec for AeolianEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.aeolian.vst3",
        name: c"Stave Aeolian",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A sympathetic string harp: twelve voices that ring with what you play.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Filter, PluginFeature::Stereo],
    };

    fn params() -> Arc<AeolianParams> {
        AeolianParams::new()
    }
    fn editor(params: &AeolianParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &AeolianParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The harp's clock tap: seconds of audio played (wrapped hourly).
pub const CLOCK_TAP: &str = "harp clock";

/// The twelve string taps, C … B — each string's ring level (0..1).
pub const LED_TAPS: [&str; 12] = [
    "ring C", "ring C#", "ring D", "ring D#", "ring E", "ring F",
    "ring F#", "ring G", "ring G#", "ring A", "ring A#", "ring B",
];

/// Everything the face reads: the twelve strings, then the clock.
pub const FACE_TAPS: [&str; 13] = [
    LED_TAPS[0], LED_TAPS[1], LED_TAPS[2], LED_TAPS[3], LED_TAPS[4], LED_TAPS[5],
    LED_TAPS[6], LED_TAPS[7], LED_TAPS[8], LED_TAPS[9], LED_TAPS[10], LED_TAPS[11],
    CLOCK_TAP,
];

/// The plugin: engine + declarations under the framework shell.
pub type Aeolian = StavePlugin<AeolianEngine>;

stave_vst3_export!(
    AeolianEngine,
    component_cid: (0x1109FB57, 0x8C770B3F, 0xDE422D02, 0x4153C104),
    controller_cid: (0x88E6EFD3, 0xF14BF1D7, 0x2EADEF58, 0x23F72554),
);

stave_clap::stave_clap_export!(AeolianEngine); // CLAP build, same cdylib
