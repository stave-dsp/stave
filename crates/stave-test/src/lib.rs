//! Testing stave code: the harness that drives a `stave::Plugin` in-process,
//! the checks every plugin mounts (validation, face bindings,
//! no-alloc, perf, stability), and — under `tests/` — the conformance
//! suites behind stave-dsp's quality claims. Measurement itself lives in
//! `stave-measure`.
//!
//!   - `plugin_checks!` every check a plugin owes, from one line
//!   - `harness`        drive a `Plugin` in-process (no FFI)
//!   - `checks`         host behaviour: block sizes, rates, cycles, automation, state
//!   - `validation`     stave-host's validation suite on a linked plugin
//!   - `face`           drive a face as a person would (`FaceDriver`), its laws
//!   - `instance`       the harness as a stave-host `Instance` (T1)
//!   - `image`          golden pictures: compare a canvas within a tolerance
//!   - `no_alloc`       no allocation on the audio thread
//!   - `perf`           the perf report
//!   - `stability`      no pumping, no runaway energy

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod checks;
pub mod face;
pub mod harness;
pub mod image;
pub mod instance;
pub mod no_alloc;
pub mod perf;
pub mod plugin_checks;
pub mod stability;
pub mod validation;

pub use face::FaceDriver;

#[doc(hidden)]
pub use assert_no_alloc as __alloc;
#[doc(hidden)]
pub use stave as __stave;
#[doc(hidden)]
pub use stave_host as __host;
pub use harness::{change, HarnessBuilder, PluginHarness};
pub use stability::{
    assert_no_pump, energy_trend, resonant_tone, StabilityConfig, StabilityReport,
};
