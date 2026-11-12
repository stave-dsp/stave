//! Tabbed analysis views. Each view borrows the shared `AnalysisRing` (for
//! audio data) and `DebugLog` (for stats / log lines), and renders its slice
//! of the central panel.

pub mod debug;
pub mod debugger;
pub mod meters;
pub mod perf;
pub mod phase;
pub mod piano;
pub mod plot;
pub mod profiles;
pub mod response;
pub mod scope;
pub mod spectral;
pub mod spectrogram;
pub mod spectrum;
pub mod validate;
