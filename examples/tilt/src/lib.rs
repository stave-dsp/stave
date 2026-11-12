//! Stave Tilt — a one-knob tilt EQ. **Start here**: this file is the
//! plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the face, a pure function of params
//! engine.rs   the ENGINE half: a shelving pair per lane, tilted
//!               around a pivot — one concept, one file
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and THE LAWS (read tests/properties.rs — it tells
//!               the story of why projects write their own)
//! ```
//!
//! STATUS: **the minimal worked example** (docs/writing-a-plugin.md
//! points here). A real tilt EQ — ±6 dB seesaw around 650 Hz —
//! small enough to read whole, complete enough to ship. Born as a
//! new-developer walkthrough test; its first build had the tilt
//! knob BACKWARDS and passed every generic gate — the sign law in
//! tests/properties.rs is the lesson, kept on purpose.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;

pub use engine::TiltEngine;
pub use params::TiltParams;

impl Spec for TiltEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.tilt.vst3",
        name: c"Stave Tilt",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/stave-dsp/stave",
        description: c"A one-knob tilt EQ.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Eq,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<TiltParams> {
        TiltParams::new()
    }
    fn face(params: &TiltParams) -> Option<stave_core::face::Face> {
        Some(editor::face(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Tilt = StavePlugin<TiltEngine>;

stave_vst3_export!(
    TiltEngine,
    component_cid: (0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700001),
    controller_cid: (0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700002),
);

stave_clap::stave_clap_export!(TiltEngine); // CLAP build, same cdylib
