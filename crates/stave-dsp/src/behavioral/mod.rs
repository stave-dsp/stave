//! Models fitted to measurements or chosen by ear, not derived from a
//! schematic: tabulated transfer curves and their anti-aliasing, shape
//! functions, and grey-box models. A part here makes no circuit claim;
//! circuit-derived parts live in [`crate::circuit`].

pub mod adaa;
pub mod curve;
pub mod hysteresis;
pub mod rc;
pub mod shapes;
pub mod slew;
pub mod smooth_sat;
