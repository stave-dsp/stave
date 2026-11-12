//! Factory presets: named sets of parameter values, declared against the
//! plugin's typed params.

use std::vec::Vec;

use crate::params::ParamId;

/// A factory preset: values (in each param's own units) keyed by
/// `ParamId`. Wrappers expose the list as the host's program list; a
/// face steps through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub values: Vec<(ParamId, f64)>,
}

impl Preset {
    /// A preset from `(id, value)` pairs.
    ///
    /// ```
    /// use stave_core::{ParamId, Preset};
    ///
    /// let warm = Preset::from_pairs("Warm", &[(ParamId(1), -3.0), (ParamId(2), 1.5)]);
    /// assert_eq!(warm.values.len(), 2);
    /// ```
    pub fn from_pairs(name: &'static str, pairs: &[(ParamId, f64)]) -> Self {
        Self {
            name,
            values: pairs.to_vec(),
        }
    }
}
