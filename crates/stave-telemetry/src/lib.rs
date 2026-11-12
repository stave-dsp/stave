//! Telemetry: the plugin → editor/tool channel of a stave plugin.
//!
//! [`Telemetry`] is a fixed-capacity, lock-free block a format wrapper
//! allocates per instance and hands to the plugin. The audio thread
//! publishes through typed [`Tap`]s; the editor and host read it. Its
//! layout is `#[repr(C)]` over atomics only, so a host shares it across a
//! dlopen boundary when both sides were built against the same
//! `stave-telemetry` ([`ABI_VERSION`] is the first field).
//!
//! ## Taps: the shipping path
//!
//! A plugin declares its taps once, as a struct; `#[derive(Taps)]` (in
//! `stave-macros`, re-exported by `stave-core`) gives each field a
//! [`TapId`] constant and a constructor that registers them all:
//!
//! ```ignore
//! #[derive(Taps)]
//! pub struct HeirloomTaps { pub spin: Tap, pub level: Tap }
//!
//! let taps = HeirloomTaps::new(&telemetry);       // at instance creation
//! taps.spin.set(angle);                          // audio thread: lock-free, O(1)
//! telemetry.value(HeirloomTaps::SPIN);           // a reader: by id, no strings
//! ```
//!
//! The engine and the face name the same constant, so a typo is a compile
//! error rather than a meter that reads 0. A name longer than a slot holds
//! ([`NAME_BYTES`] − 1 bytes) is a compile error too.
//!
//! ## Probes and breakpoints: tooling only
//!
//! The string-keyed [`probe!`] and [`brk_if!`] macros remain for harness,
//! bench and debugging code (stave-play's debugger lists whatever they
//! publish). Shipping engines and faces use taps.
//!
//! ## Also on the block
//!
//! - **Capture**: the latest block of output per channel, written by the
//!   wrapper (or by the plugin after [`Telemetry::claim_capture`]).
//! - **Curve**: a published transfer-curve table ([`Telemetry::publish_curve`]).
//! - **Messages**: the plugin's recent log lines ([`Messages`]).
//! - **Events**: the instance's event trace — lifecycle, gestures,
//!   restores, faults — as typed records a test or tool checks against
//!   the protocol's laws ([`events`]).
//! - **Pause**: an armed breakpoint that fires sets `paused`; the wrapper
//!   then writes silence while the plugin keeps running (a real halt would
//!   underrun), so its state stays observable.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod breakpoint;
mod capture;
pub mod events;
mod inline_str;
mod limits;
#[macro_use]
mod macros;
mod messages;
mod scope;
mod slot;
mod sync;
mod tap;
mod telemetry;
#[cfg(test)]
mod tests;

pub use self::breakpoint::Breakpoint;
pub use self::capture::Capture;
pub use self::events::{Event, Events, Fault};
pub use self::limits::*;
pub use self::messages::Messages;
pub use self::scope::Scope;
pub use self::tap::{Tap, TapId, TapReading};
pub use self::telemetry::Telemetry;

/// The lock-free pieces the loom models (`tests/loom.rs`) check, built on
/// loom's atomics under `cfg(loom)`: the shipping types, not copies.
#[cfg(loom)]
#[doc(hidden)]
pub mod model {
    pub use crate::inline_str::InlineStr;
    pub use crate::messages::Message;
    pub use crate::slot::{reserve_index, TapSlot};
}
