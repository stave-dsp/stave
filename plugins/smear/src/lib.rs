//! Stave Smear — "Sympathetic Garden". **Start here**: this file is
//! the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports (stereo, with a 5.1
//!               surround variant), params, editor — the
//!               DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! engine/     the ENGINE half:
//!   mod.rs    the spine: the process loop + drift updates
//!   garden.rs the resonator lattice (one garden per lane) — a
//!               LEAF: dense cross-coupled cores stay whole
//! tests/      this project's properties: the garden suites, the
//!               surround lift bit-law, validation, editor bindings,
//!               the no-alloc check
//! ```
//!
//! A field of tuned resonators (sympathetic strings) on a chromatic
//! pitch lattice. Your input excites them the way an open piano
//! rings when you sing into it; energy blooms across neighbouring
//! pitches (the "smear"), tunings drift so the garden is alive, and
//! a saturated regeneration path sustains into pads or drones.
//! Entirely TIME-DOMAIN: real filters, real outputs — it rings at
//! actual notes and cannot collapse into noise like a spectral
//! engine.
//!
//! Controls: SEED (input drive), SMEAR (neighbour coupling), BLOOM
//! (regeneration), DECAY (ring time), FLUX (drift rate), SHIMMER
//! (octave bloom), DRIVE (feedback warmth), MOTION (detune depth),
//! DEPTH (lane spread), TONE (HF damping), MIX, OUTPUT.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{Arrangement, PluginDescriptor, PluginFeature, PortConfig};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;

pub use engine::SmearEngine;
pub use params::SmearParams;

impl stave_core::Spec for SmearEngine {
    /// the declared set — stereo preferred, 5.1 as the surround
    /// variant of the PerChannel lift. The gardens are per-lane
    /// independent, so the lift is exactly what Lanes are for; the
    /// stereo pair is bit-pinned through it.
    const CONFIGS: &'static [PortConfig] = &[
        PortConfig::STEREO_FX,
        PortConfig {
            main_in: Some(Arrangement::Surround51),
            main_out: Arrangement::Surround51,
            aux_in: &[],
        },
    ];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.smear.vst3",
        name: c"Stave Smear",
        vendor: c"Ryan Bahan",
        version: c"2.0.0",
        url: c"https://github.com/stave-dsp/stave",
        description:
            c"Sympathetic Garden — self-tuning resonator field; smear blooms across pitches.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Reverb,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<SmearParams> {
        Arc::new(SmearParams::new())
    }
    fn editor(params: &SmearParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Smear = stave_core::StavePlugin<SmearEngine>;

stave_vst3_export!(
    SmearEngine,
    component_cid: (0x5A7FDD1D, 0x6E5E4F88, 0x9D6D7E11, 0x55E76BF1),
    controller_cid: (0x5A7FDD1D, 0x6E5E4F88, 0x9D6D7E11, 0x55E76BF2),
);

stave_clap::stave_clap_export!(SmearEngine); // CLAP build, same cdylib
