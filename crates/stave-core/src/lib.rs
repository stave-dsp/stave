//! Core audio plugin abstractions for the stave framework.
//!
//! Defines the host-agnostic `Plugin` trait that audio modules implement.
//! Backend crates (`stave-clap`, future `stave-au`, `stave-standalone`) adapt
//! this trait to their respective ABIs.

#![deny(rust_2018_idioms)]

// The derives name `::stave_core`; inside this crate that is `crate`.
extern crate self as stave_core;

pub mod block;
pub mod buffer;
pub mod diagnostics;
pub mod environment;
pub mod event;
pub mod latency;
pub mod params;
pub mod plugin;
pub mod preset;
pub mod process;
pub mod processor;
pub mod state;
pub mod wrapper;

pub use block::{LaneFrame, Lanes, Peaks};
pub use buffer::AudioBuffer;
pub use environment::Environment;
pub use event::{Event, EventCursor, EventKind, NoteEvent, NoteKind};
pub use params::ParamId;
pub use plugin::{Arrangement, AuxIn, Plugin, PluginDescriptor, PluginFeature, PortConfig, Tail};
pub use preset::Preset;
pub use process::{ParamBlock, ParamChange, ParamChangeKind, ProcessCtx, Transport};
pub use processor::{Processor, Spec, StavePlugin};
pub use stave_macros::Taps;
pub use telemetry::{Scope, Tap, TapId, Telemetry};
pub use stave_face as face;
pub use wrapper::{
    ranges_alias, GestureEvent, GestureQueue, PluginCell, ProcessorGuard, ProcessorState,
    ScopedFtz, SharedState,
};
// The tooling macros at the crate root: `use stave_core::{probe, brk_if};`.
pub use stave_telemetry::{brk_if, probe};

/// The plugin → editor/tool channel (`stave-telemetry`), with
/// `#[derive(Taps)]`: declare a struct of [`Tap`]s, register it when the
/// plugin is created, publish from `process`, read by [`TapId`].
///
/// ```
/// use stave_core::telemetry::{Tap, Taps, Telemetry};
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
/// use stave_core::telemetry::{Tap, Taps};
///
/// #[derive(Taps)]
/// pub struct Long { pub a_tap_name_thirty_two_bytes_long: Tap }
/// ```
pub mod telemetry {
    pub use stave_macros::Taps;
    pub use stave_telemetry::*;
}
