//! Testing stave code: the harness that drives a `stave::Plugin` in-process,
//! the checks every plugin mounts (validation, face bindings,
//! no-alloc, perf, stability), and — under `tests/` — the conformance
//! suites behind stave-dsp's quality claims. Measurement itself lives in
//! `stave-measure`.
//!
//!   - `harness`        drive a `Plugin` in-process (no FFI)
//!   - `validation`     stave-host's validation suite on a linked plugin
//!   - `face`           drive a face as a person would (`FaceDriver`), its laws
//!   - `image`          golden pictures: compare a canvas within a tolerance
//!   - `no_alloc`       no allocation on the audio thread
//!   - `perf`           the perf report
//!   - `stability`      no pumping, no runaway energy

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod face;
pub mod harness;
pub mod image;
pub mod no_alloc;
pub mod perf;
pub mod stability;
pub mod validation;

pub use face::FaceDriver;
pub use harness::PluginHarness;
pub use stability::{
    assert_no_pump, energy_trend, resonant_tone, StabilityPolicy, StabilityReport,
};
