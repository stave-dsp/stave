//! VST3 plugin hosting: load a bundle (or statically linked entry
//! points), drive its processor, reach its controller. A
//! [`LoadedPlugin`] is an [`Instance`](crate::instance::Instance), so
//! everything that measures plugins works on it unchanged.
//!
//! ```text
//! bundle.rs      .vst3 on disk → CFBundle → executable
//! module.rs      entry points; bundleEntry/Exit; dlopen/dlclose
//! factory.rs     classes; picking the audio module
//! open.rs        the load sequence (traced, guard per step)
//! loaded.rs      the live instance: process, state, teardown
//! controller.rs  parameters: inventory, writes, edits to the processor
//! telemetry.rs   a stave plugin's telemetry: taps, messages
//! transport.rs   the ProcessContext plugins see
//! trace.rs       the load trace (STAVE_HOST_TRACE=1 streams it)
//! com/           host-implemented COM objects + the Com<T> pointer
//! ```

mod bundle;
mod com;
mod controller;
mod error;
mod factory;
mod loaded;
mod module;
mod open;
mod telemetry;
mod trace;
mod transport;

pub use bundle::Bundle;
pub use controller::ControllerHandle;
pub use error::HostError;
pub use factory::{ClassInfo, AUDIO_MODULE_CLASS};
pub use loaded::{ControllerKind, HostStats, LoadedPlugin, PluginInfo};
pub use module::EntryPoints;
pub use trace::{LoadTrace, TraceStep};
pub use transport::TransportConfig;
