//! The parameter system.
//!
//! Everything wire-visible about a parameter flows from one declarative
//! source: a stable author-chosen id, a [`Range`] with a closed
//! [`Taper`] vocabulary, a [`Unit`] that formats and parses, and a
//! declared [`Smooth`] that the processor's controls render. Wrappers,
//! editors, and state chunks all consume the same table — parity is
//! structural.

mod control;
mod display;
mod route;
mod smooth;
mod taper;
mod types;
mod watch;

pub use display::{Unit, ValueText, VALUE_TEXT_BYTES};
pub use route::{RouteMode, RouteParams, RouteQuant, MAX_ROUTES, ROUTE_MODES, ROUTE_QUANTS};
pub use smooth::Smooth;
pub use taper::{Range, Taper};
pub use types::{
    AnyParam, BoolParam, EnumParam, FloatParam, IndexedParams, NoParams, ParamEnum, ParamFlags, ParamMeta,
    ParamTableError, Params,
};
pub use watch::{ModalWatch, Watch};

/// The derive lives in its own proc-macro crate; re-exported here so
/// `use stave_core::params::Params;` brings both the trait and the
/// derive (they occupy different namespaces, like `fmt::Debug`).
pub use stave_macros::Params;

/// Stable parameter identity — FOREVER once shipped (state, hosts,
/// and editors key on it). Folded from the retired pre-table
/// `stave_core::param` module.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamId(pub u32);

impl ParamId {
    /// The program-change parameter wrappers add for a plugin's factory
    /// presets (VST3 `kIsProgramChange`). Reserved: no table may use it.
    pub const PROGRAM: ParamId = ParamId(0x7FFF_FFFF);
}
pub use control::{
    ContinuousBlock, ContinuousControl, DiscreteBlock, DiscreteChange, DiscreteControl,
    ReconfigureControl,
};

/// A parameter as a face binds it: `p.drive.bind()`.
pub trait BindParam {
    fn bind(&self) -> stave_face::Bind;
}

impl<T: AnyParam + ?Sized> BindParam for T {
    fn bind(&self) -> stave_face::Bind {
        let m = self.meta();
        stave_face::Bind::new(m.id.0, m.name)
    }
}
