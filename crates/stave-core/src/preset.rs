//! Factory presets: named sets of plain parameter values, declared
//! against the plugin's typed params.

extern crate alloc;
use alloc::vec::Vec;

use crate::params::ParamId;

/// A factory preset: plain values keyed by `ParamId`. Wrappers expose
/// the list as the host's program list; a face steps through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub values: Vec<(ParamId, f64)>,
}
