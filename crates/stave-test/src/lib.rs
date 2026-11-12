//! Testing stave code: the harness that drives a `Plugin` in-process,
//! the checks every plugin mounts (validation, editor bindings,
//! no-alloc, perf, stability), and — under `tests/` — the conformance
//! suites behind stave-dsp's quality claims. Measurement itself lives in
//! `stave-measure`.
//!
//!   - `harness`        drive a `Plugin` impl in-process (no FFI)
//!   - `validation`     stave-host's validation suite on a linked plugin
//!   - `editor`         editor ↔ parameter binding proofs
//!   - `no_alloc`       no allocation on the audio thread
//!   - `perf`           the perf report
//!   - `stability`      no pumping, no runaway energy

#![deny(rust_2018_idioms)]

pub mod editor;
pub mod harness;
pub mod no_alloc;
pub mod perf;
#[cfg(target_os = "macos")]
pub mod raster;
pub mod stability;
pub mod validation;

pub use harness::PluginHarness;
pub use stability::{
    assert_no_pump, energy_trend, resonant_tone, StabilityPolicy, StabilityReport,
};
