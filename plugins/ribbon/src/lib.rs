//! Stave Ribbon — an impossible tape echo, built the way the machine
//! is built: a loop of tape modelled in TAPE coordinates (erase head,
//! record head, three playback heads, a motor with inertia), a 12AX7
//! in front of the record head, a dispersive two-spring tank. The
//! impossible part is the ERASE knob: turn it down and the loop keeps
//! what it held — every pass records on top of the last, each
//! generation duller, sound-on-sound into a slowly decaying archive.
//! Behind an oxblood face whose smoked window shows the loop running.
//! **Start here**: this file is the plugin's root; everything below is
//! one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  factory presets
//! editor.rs   the face: one photoreal scene
//! pedal/      the face's picture: the bake (render.rs) + the live
//!               machine and controls (mod.rs)
//! engine/     the ENGINE half:
//!   mod.rs        the spine: bind → render → observe
//!   tape.rs       Tape — the loop, the heads, the motor
//!   preamp.rs     Preamp — the 12AX7 into the record head
//!   spring.rs     Spring — the dispersive two-spring tank
//!   telemetry.rs  Telemetry — where the loop is, how fast it runs
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

pub use engine::RibbonEngine;
pub use params::RibbonParams;

impl Spec for RibbonEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.ribbon.vst3",
        name: c"Stave Ribbon",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"An impossible tape echo: a real loop, an erase head you can turn down.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Delay, PluginFeature::Reverb, PluginFeature::Stereo],
    };

    fn params() -> Arc<RibbonParams> {
        RibbonParams::new()
    }
    fn editor(params: &RibbonParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &RibbonParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// Where the loop is, 0..1 of a turn (the face runs the ribbon by it).
pub const LOOP_TAP: &str = "tape loop";
/// The motor's speed (1 = nominal).
pub const SPEED_TAP: &str = "tape speed";

/// The plugin: engine + declarations under the framework shell.
pub type Ribbon = StavePlugin<RibbonEngine>;

stave_vst3_export!(
    RibbonEngine,
    component_cid: (0x57D986BC, 0x6E8207EB, 0x51A73C99, 0xD1CD583C),
    controller_cid: (0x78955EAB, 0x3D6B1CFC, 0xE191FB7F, 0xCF075A71),
);

stave_clap::stave_clap_export!(RibbonEngine); // CLAP build, same cdylib
