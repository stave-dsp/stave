//! In-process debugger for stave plugins.
//!
//! `DebugBus` is a fixed-capacity, lock-free state container that a plugin
//! attaches to during instance creation. The audio thread writes through
//! declared [`Tap`]s and `brk_if!`; the editor and host read the bus
//! directly. Layout is `#[repr(C)]` with only atomic primitives inside so
//! a host can share the bus across a dlopen boundary as long as both sides
//! were built against the same `stave-debug` version (workspace-pinned).
//!
//! ## What it supports
//!
//! - **Probes**: named scalar values (f64) sampled per-block by the plugin.
//!   Each probe tracks `latest`, `min`, `max`, and a hit counter.
//! - **Breakpoints**: named arm/condition pairs. When a `brk_if!` macro
//!   evaluates true and the breakpoint is armed, the bus records a hit and
//!   optionally raises the global `paused` flag, which the plugin's format wrapper
//!   should treat as "mute the output until cleared".
//! - **Capture frame**: the most recent block worth of output samples per
//!   channel, written by the wrapper each block. The GUI reads this for an
//!   inspector view.
//!
//! ## What it deliberately doesn't do
//!
//! - Stop the audio thread. Real "halt" would underrun. "Paused" means the
//!   wrapper writes silence; the plugin's internal state stays observable.

extern crate alloc;

mod breakpoint;
mod bus;
mod capture;
mod inline_str;
mod limits;
#[macro_use]
mod macros;
mod messages;
mod probe;
mod scope;
mod tap;
#[cfg(test)]
mod tests;

pub use self::breakpoint::Breakpoint;
pub use self::bus::DebugBus;
pub use self::capture::CaptureFrame;
pub use self::inline_str::InlineStr;
pub use self::limits::*;
pub use self::messages::{Message, Messages};
pub use self::probe::Probe;
pub use self::scope::Scope;
pub use self::tap::Tap;
