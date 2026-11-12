//! Stave Vault. **Start here**: this file is the plugin's
//! root; everything below is one click away.
//!
//! ```text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! presets.rs  factory presets — the character space's corners
//! editor.rs   the UI tree, a pure function of params
//! engine/     the ENGINE half: mod.rs orchestration, tank.rs (the
//!               8-line Hadamard FDN), diffuser.rs (input allpasses)
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and the reverb's OWN laws (properties.rs — T60 is
//!               a measured promise, passivity, bit-dry mix zero)
//! ```
//!
//! STATUS: plugins/vault/docs/design.md — predelay → image-source ROOM (early) →
//! diffused second-order feed → 8-line FDN TANK (late) + MODAL
//! bank (LF standing waves) → lean/width/duck field → bit-dry mix.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;
mod presets;

pub use engine::VaultEngine;
pub use params::VaultParams;

impl Spec for VaultEngine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.ryanbahan.stave.vault.vst3",
        name: c"Stave Vault",
        vendor: c"Ryan Bahan",
        // Bumped per shipped slice — the field must be able to ask
        // a DAW "which vault is this?" (0.10.2 = shelf-cascade loop
        // filters: passive at EVERY dial).
        version: c"0.37.1",
        url: c"https://github.com/ryancbahan/stave",
        description: c"Stave Vault — a place, not an algorithm.",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<VaultParams> {
        VaultParams::new()
    }
    fn editor(params: &VaultParams) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
    fn presets(params: &VaultParams) -> alloc::vec::Vec<stave_core::Preset> {
        presets::presets(params)
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type Vault = StavePlugin<VaultEngine>;

stave_vst3_export!(
    VaultEngine,
    component_cid: (0xB08D3A3D, 0x7965C2F2, 0x6A8988F6, 0x28E796EC),
    controller_cid: (0x937A3619, 0x0C719553, 0x091740BA, 0xD89EAFD4),
);

stave_clap::stave_clap_export!(VaultEngine); // CLAP build, same cdylib
