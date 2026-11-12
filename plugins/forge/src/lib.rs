//! Stave Forge — an amp-topology saturator. **Start here**: this
//! file is the plugin's root; everything below is one click away.
//!
//! ```text
//! lib.rs        ← you are here: identity, ports, params, editor,
//!                 presets — the DECLARATIONS half, no behavior
//! params.rs     every knob, with its forever-stable id
//! editor.rs     the UI tree, a pure function of params
//! presets.rs    factory presets, pure function of params
//! engine/          the ENGINE half — the realtime processor
//!   mod.rs         the component struct + `process`, the whole
//!                  per-block orchestration in three named phases
//!   strip.rs       `Strip` — one M/S lane: topology + 4× region
//!   bindings.rs    `Bindings::apply_changes` — params → structure
//!   modulation.rs  `Modulation` — drivers, routes, displacement
//!   process.rs     `process_block` — the audio block
//!   telemetry.rs   `Telemetry::publish` — taps, scope, editor feeds
//!   voicing.rs     the sound-design vocabulary (curve recipes)
//! tests/        this project's properties: voicing suites, taps,
//!               validation, editor bindings, the no-alloc check
//! ```
//!
//! What it sounds like — the breadth axes, exposed as DISCRETE
//! selectors (proper composition, not one morph knob):
//!
//! * CHARACTER — the harmonic recipe of the primary nonlinearity. Six
//!   ladders with genuinely different even/odd content and falloff
//!   (Chebyshev weight sets, knee families, and blends). EDGE morphs
//!   WITHIN the chosen family.
//! * STACK — the circuit topology. Single stage, two-stage cascade,
//!   screamer (a real feedback-clipper RC first — threshold rides on
//!   charge history), or push-pull (opposed-bias pair: crossover-ish
//!   odd emphasis).
//! * FILTER / POST FILTER — two explicit filter selectors. FILTER sits
//!   BETWEEN the stages (flat/scoop/bump/bright/dark/honk placements —
//!   where the G(f, level) bend comes from; TONE trims it). POST FILTER
//!   shapes the output (flat/air/low cut/warm/hi cut/dark), named as
//!   the EQ moves they are.
//! * MEMORY — the program-dependent behaviour: none, supply sag
//!   (drive give + darkening), grid blocking (sense-driven bias choke),
//!   squeeze (envelope gain compression), or ripple (env-scaled 120 Hz
//!   ghost sidebands). DEPTH scales it.
//! * SUB — the SubOctave divider, both roles: SUB MIX feeds the locked
//!   octave-down into the chain input (saturated by the same circuit);
//!   SUB MOD gain-modulates the primary stage at f0/2, putting energy
//!   at 1.5·f0/2.5·f0 — between the harmonics.
//!
//! Everything runs inside the nested 4× FIR stack; the chain is
//! prepared at 4× the host rate (rate contract); latency is the
//! constant 84 samples with a compensated dry path. M/S: the mid chain
//! always gets full drive; WIDTH scales the side chain.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;

use stave_core::{PluginDescriptor, PluginFeature, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;
mod presets;

pub use engine::ForgeEngine;
use params::ForgeParams;

impl Spec for ForgeEngine {
    /// Stereo FX, stated (the default is the same — the flagship
    /// declares its negotiation surface rather than inheriting it).
    const CONFIGS: &'static [stave_core::PortConfig] = &[stave_core::PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.forge.vst3",
        name: c"Stave Forge",
        vendor: c"Ryan Bahan",
        version: c"0.0.2",
        url: c"https://github.com/ryancbahan/stave",
        description: c"Amp-topology saturation: discrete characters, stacks, voices and memory.",
        features: &[
            PluginFeature::AudioEffect,
            PluginFeature::Distortion,
            PluginFeature::Stereo,
        ],
    };

    fn params() -> Arc<ForgeParams> {
        ForgeParams::new()
    }
    fn editor(params: &ForgeParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &ForgeParams) -> Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Forge = StavePlugin<ForgeEngine>;

stave_vst3_export!(
    ForgeEngine,
    component_cid: (0x5A7FE101, 0x6E5E4F88, 0x9D6D7E11, 0x55E82001),
    controller_cid: (0x5A7FE101, 0x6E5E4F88, 0x9D6D7E11, 0x55E82002),
);

stave_clap::stave_clap_export!(ForgeEngine); // CLAP build, same cdylib
