//! Testing stave code: the harness that drives a `stave::Plugin` in-process,
//! the checks every plugin mounts (validation, face bindings,
//! no-alloc, perf, stability), and — under `tests/` — the conformance
//! suites behind stave-dsp's quality claims. Measurement itself lives in
//! `stave-measure`.
//!
//!   - `plugin_checks!` every check a plugin owes, from one line
//!   - `harness`        drive a `Plugin` in-process (no FFI)
//!   - `checks`         host behaviour: block sizes, rates, cycles, automation, state
//!   - `characterise`   a circuit's characterisation matrix against its reference (DSP.md R17)
//!   - `corpus`         reference program material by role from the sample library (DSP.md R22, C13)
//!   - `validation`     stave-host's validation suite on a linked plugin
//!   - `face`           drive a face as a person would (`FaceDriver`), its laws
//!   - `instance`       the harness as a stave-host `Instance` (T1)
//!   - `image`          golden pictures: compare a canvas within a tolerance
//!   - `no_alloc`       no allocation on the audio thread
//!   - `perf`           the perf check against a plugin's CPU budget
//!   - `reference`      a model against ngspice, a solved reference or a datasheet curve, within a budget
//!   - `stability`      no pumping, no runaway energy

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod characterise;
pub mod checks;
pub mod corpus;
pub mod face;
pub mod harness;
pub mod image;
pub mod instance;
pub mod no_alloc;
pub mod perf;
pub mod plugin_checks;
pub mod reference;
pub mod sha256;
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

/// The threads a test's data-parallel work may use: `RAYON_NUM_THREADS`,
/// else `STAVE_OFFLOAD_JOBS` (a build host's per-job share), else the
/// machine's parallelism — what a design study's `Bench::threads` takes.
pub fn threads() -> usize {
    ["RAYON_NUM_THREADS", "STAVE_OFFLOAD_JOBS"]
        .iter()
        .find_map(|k| std::env::var(k).ok()?.parse::<usize>().ok().filter(|&n| n > 0))
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}
pub use stability::{
    assert_no_pump, energy_trend, resonant_tone, StabilityConfig, StabilityReport,
};
