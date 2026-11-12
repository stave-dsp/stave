//! Stave Multiband — a 3-band compressor. **Start here**: this file
//! is the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs        identity, ports (stereo + an external-sidechain aux)
//! params.rs     every knob, with its forever-stable id
//! style.rs      compressor styles (VCA/FET/OPTO/VMU) and their
//!               program dependence
//! detector.rs   a band's detector: HPF, peak/RMS, retimed follower
//! saturation.rs the per-band saturator
//! editor.rs     the UI tree, a pure function of params
//! engine/
//!   mod.rs      the split, the bands, the sum
//!   band.rs     one band: controls, gain, saturation, blend
//!   splitter.rs the LR4 band split (main path and sidechain)
//!   dynamics.rs the gain computer
//! tests/        sidechain, validation, editor bindings, no-alloc
//! ```
//!
//! Topology: two Linkwitz-Riley 4th-order crossovers split the input
//! into low/mid/high; each band runs an independent feedforward
//! compressor with per-band character; the bands sum back (LR4
//! split-and-sum is unity-gain, allpass phase at the crossovers).

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{Arrangement, AuxIn, PluginDescriptor, PluginFeature, PortConfig};
use stave_vst3::stave_vst3_export;

pub mod detector;
mod editor;
mod engine;
mod params;
#[doc(hidden)]
pub mod saturation;
pub mod style;

pub use engine::MultibandEngine;
use params::MbParams;

impl stave_core::Spec for MultibandEngine {
    /// The declared configuration SET, preference order.
    /// Member 0 = stereo + external-sidechain aux; member 1 = plain
    /// stereo, for hosts that won't wire an aux. The whole tuple is
    /// the negotiation unit — a host picks a member (VST3
    /// setBusArrangements lookup, CLAP audio-ports-config select),
    /// never probes a predicate.
    const CONFIGS: &'static [PortConfig] = &[
        PortConfig {
            main_in: Some(Arrangement::Stereo),
            main_out: Arrangement::Stereo,
            aux_in: &[AuxIn {
                name: "sidechain",
                arrangement: Arrangement::Stereo,
            }],
        },
        PortConfig::STEREO_FX,
    ];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.multibandcomp.vst3",
        name: c"Stave Multiband",
        vendor: c"Ryan Bahan",
        version: c"0.0.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"3-band multiband compressor with Linkwitz-Riley crossover.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<MbParams> {
        MbParams::new()
    }
    fn editor(params: &MbParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type MultibandComp = stave_core::StavePlugin<MultibandEngine>;

stave_vst3_export!(
    MultibandEngine,
    component_cid: (0x5A7FCC1C, 0x6E5E4F88, 0x9D6D7E11, 0x55E76BE1),
    controller_cid: (0x5A7FCC1C, 0x6E5E4F88, 0x9D6D7E11, 0x55E76BE2),
);

stave_clap::stave_clap_export!(MultibandEngine); // CLAP build, same cdylib
