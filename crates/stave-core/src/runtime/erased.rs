//! The plugin contract the format wrappers drive, with every type
//! erased that a wrapper does not need: params as a table, presets as a
//! shared list. `stave::export!` implements it for an author's
//! `stave::Plugin`; the wrappers' own tests implement it directly.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;

use stave_telemetry::Telemetry;

use crate::environment::Environment;
use crate::info::{Info, Layout};
use crate::params::Params;
use crate::preset::Preset;
use crate::process::Block;
use crate::state::Migrations;
use crate::Tail;

/// A plugin as the format wrappers see it. Mirrors `stave::Plugin`, with
/// the params erased to `dyn Params`.
pub trait Erased: Send + Sized + 'static {
    const INFO: Info;
    /// Never empty; the first is preferred.
    const LAYOUTS: &'static [Layout] = &[Layout::STEREO];
    /// Written into every state chunk; `migrations` maps older versions.
    const STATE_VERSION: u32 = 0;

    /// A fresh instance, publishing its taps on `telemetry`.
    fn new(telemetry: &Telemetry) -> Self;

    /// The parameter table, behind an id index: the same `Arc` every
    /// call (wrappers serve every host thread from it).
    fn params(&self) -> Arc<dyn Params> {
        Arc::new(crate::params::NoParams)
    }

    /// Factory presets, one list shared by the wrapper and every face.
    fn presets(&self) -> Arc<[Preset]> {
        Arc::from(Vec::new())
    }

    /// Before processing, and again when the conditions change. May
    /// allocate.
    fn prepare(&mut self, _env: &Environment) {}

    /// Clear DSP state without touching params. Realtime.
    fn reset(&mut self) {}

    /// Realtime.
    fn process(&mut self, block: &mut Block<'_>);

    /// Samples of delay the host compensates.
    fn latency(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::None
    }

    fn migrations() -> Migrations {
        Migrations::default()
    }

    /// State beyond params, for the session chunk.
    fn save(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Restore `save`'s bytes, written at state `version`; params are
    /// already applied.
    fn load(&mut self, _bytes: &[u8], _version: u32) {}
}

/// A tail as hosts encode it (VST3 `getTailSamples`, `clap.tail`): 0 for
/// none, `u32::MAX` for infinite.
pub fn tail_wire(tail: Tail) -> u32 {
    tail.to_samples().unwrap_or(u32::MAX)
}
