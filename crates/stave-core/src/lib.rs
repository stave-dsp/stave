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
pub mod editor;
pub mod editor_kit;
pub mod environment;
pub mod event;
pub mod latency;
pub mod params;
pub mod plugin;
pub mod process;
pub mod processor;
pub mod state;
pub mod wrapper;

pub use block::{LaneFrame, Lanes, Peaks};
pub use buffer::AudioBuffer;
pub use editor::{Preset, SpectrumKeys, TapLabel, TapPill, TransferKeys, UiNode, Widget};
pub use environment::Environment;
pub use event::{Event, EventCursor, EventKind, NoteEvent, NoteKind};
pub use params::ParamId;
pub use plugin::{Arrangement, AuxIn, Plugin, PluginDescriptor, PluginFeature, PortConfig, Tail};
pub use process::{ParamBlock, ParamChange, ParamChangeKind, ProcessCtx, Transport};
pub use processor::{Processor, Spec, StavePlugin};
pub use stave_debug as debug;
pub use stave_debug::{DebugBus, Scope, Tap};
pub use stave_ui as ui;
pub use wrapper::{
    ranges_alias, GestureEvent, GestureQueue, PluginCell, ProcessorGuard, ProcessorState,
    ScopedFtz, SharedState,
};
// Re-export the debug macros at the crate root so plugins can do
// `use stave_core::{probe, brk_if};` without a direct stave-debug dep.
pub use stave_debug::{brk_if, probe};
