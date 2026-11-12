//! The stave plugin model: parameters, blocks, events, state, presets,
//! and the editor contract ([`Controller`]). UI-free and format-free.
//!
//! Authors depend on the `stave` crate, which re-exports what they need
//! from here beside the DSP and the face. The format wrappers drive a
//! plugin through [`runtime`] (not author API).

#![deny(rust_2018_idioms)]

// The derives name `::stave_core`; inside this crate that is `crate`.
extern crate self as stave_core;

pub mod block;
pub mod buffer;
pub mod controller;
pub mod diagnostics;
pub mod environment;
pub mod event;
pub mod info;
pub mod latency;
pub mod params;
pub mod preset;
pub mod process;
#[doc(hidden)]
pub mod runtime;
pub mod state;
pub mod units;

pub use block::{LaneFrame, Lanes, Peaks};
pub use buffer::{InputBuffer, OutputBuffer};
pub use controller::{Controller, GestureSink, PluginController};
pub use environment::Environment;
pub use event::{Event, EventCursor, EventKind, NoteEvent, NoteKind};
pub use info::{Aux, Channels, Feature, Info, Layout};
pub use params::ParamId;
pub use preset::Preset;
pub use process::{Block, ParamBlock, ParamChange, ParamChangeKind, Transport};
pub use stave_dsp::Tail;
/// `#[derive(Params)]`: the parameter table of a struct of params.
pub use stave_macros::Params;
/// `#[derive(Smoothers)]`: a group of [`params::Smoother`]s, one per param.
pub use stave_macros::Smoothers;
/// `#[derive(Taps)]`: the telemetry table of a struct of taps.
pub use stave_macros::Taps;
pub use telemetry::{Scope, Tap, TapId, Telemetry};
pub use units::Normalized;
// The tooling macros at the crate root: `use stave_core::{probe, brk_if};`.
pub use stave_telemetry::{brk_if, probe};

/// The plugin → face/tool channel (`stave-telemetry`), with
/// `#[derive(Taps)]`: declare a struct of [`Tap`]s, register it when the
/// plugin is created, publish from `process`, read by [`TapId`].
///
/// ```
/// use stave_core::telemetry::{Tap, Telemetry};
/// use stave_core::Taps;
///
/// #[derive(Taps)]
/// pub struct HeirloomTaps { pub spin: Tap, pub level: Tap }
///
/// let telemetry = Telemetry::new();
/// let taps = HeirloomTaps::new(&telemetry);  // when the plugin is created
/// taps.spin.set(0.25);                       // audio thread: lock-free
/// assert_eq!(telemetry.value(HeirloomTaps::SPIN), Some(0.25));
/// assert_eq!(HeirloomTaps::ALL, [HeirloomTaps::SPIN, HeirloomTaps::LEVEL]);
/// ```
///
/// A tap name longer than a slot holds is a compile error:
///
/// ```compile_fail
/// use stave_core::telemetry::Tap;
/// use stave_core::Taps;
///
/// #[derive(Taps)]
/// pub struct Long { pub a_tap_name_thirty_two_bytes_long: Tap }
/// ```
pub mod telemetry {
    /// `#[derive(Params)]`: the parameter table of a struct of params.
pub use stave_macros::Params;
/// `#[derive(Taps)]`: the telemetry table of a struct of taps.
pub use stave_macros::Taps;
    pub use stave_telemetry::*;
}
