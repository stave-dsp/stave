//! Stave Saga — a stereo reverse/reverb soundscape generator: two
//! time machines in series, each with its own feedback path, on a
//! 32 kHz clock you can pull. Modelled on the Walrus Audio Lore
//! (MKII) — docs/saga.md maps every pedal control to its param.
//! **Start here**: this file is the plugin's root; everything below
//! is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  the pedal's RED / GREEN / BLUE + postcards
//! editor.rs   the UI tree, a pure function of params
//! engine/     the ENGINE half:
//!   mod.rs        the spine: bind → render → observe
//!   motion.rs     Motion — the clock: base, DIVE/RISE, MOD LFO
//!   clock.rs      ClockDomain — host rate ⇄ the core clock
//!   programs.rs   Programs — the five wirings of the cells:
//!   reverse.rs    ReverseLine — a line read backward
//!   tank.rs       Tank — the 8-line FDN reverb
//!   transpose.rs  Transposer — the two-head pitch shifter
//!   path.rs       AnalogPath — the feedback loops' analog legs
//!   wet.rs        Wet — TONE, WIDTH, the soft ceiling
//!   telemetry.rs  Telemetry — clock + tail out
//! tests/      the shared checks + THE PROPERTIES (properties.rs: bit-dry mix,
//!               reversal is real, octaves land, the dive drops an
//!               octave, programs are distinct, maxed loops stay
//!               bounded)
//! ```

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;
mod presets;

pub use engine::SagaEngine;
pub use params::SagaParams;

impl Spec for SagaEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.saga.vst3",
        name: c"Stave Saga",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"Reverse delays and reverbs in series, on a clock you can pull.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Reverb,
            PluginFeature::Delay,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<SagaParams> {
        SagaParams::new()
    }
    fn editor(params: &SagaParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &SagaParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Saga = StavePlugin<SagaEngine>;

stave_vst3_export!(
    SagaEngine,
    component_cid: (0xC0765B4B, 0x46762B10, 0x8645B824, 0x889E2C27),
    controller_cid: (0x039D76AD, 0x9D871B51, 0x582038D9, 0xEA200920),
);

stave_clap::stave_clap_export!(SagaEngine); // CLAP build, same cdylib
