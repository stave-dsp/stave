//! The parameter system.
//!
//! Everything wire-visible about a parameter flows from one declarative
//! source: a stable author-chosen id, a [`Range`] with a closed
//! [`Taper`] vocabulary, a [`Unit`] that formats and parses, and a
//! declared [`Smooth`] that the plugin's [`Smoother`] runs. Wrappers,
//! editors, and state chunks all consume the same table — parity is
//! structural.

mod control;
mod display;
mod quantity;
mod smooth;
mod taper;
mod types;
mod watch;

pub use display::{Unit, ValueText, VALUE_TEXT_BYTES};
pub use quantity::Quantity;
pub use smooth::Smooth;
pub use taper::{Range, Taper};
pub use types::{
    AnyParam, BoolParam, EnumParam, FloatParam, IndexedParams, NoParams, ParamEnum, ParamFlags, ParamMeta,
    ParamTableError, Params,
};
pub use watch::{ModalWatch, Watch};

/// Stable parameter identity — FOREVER once shipped (state, hosts,
/// and faces key on it).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamId(pub u32);

impl ParamId {
    /// The program-change parameter wrappers add for a plugin's factory
    /// presets (VST3 `kIsProgramChange`). Reserved: no table may use it.
    pub const PROGRAM: ParamId = ParamId(0x7FFF_FFFF);
}
pub use control::{Deferred, Ramp, Smoother, Step, StepChanges, Stepped, Steps};
