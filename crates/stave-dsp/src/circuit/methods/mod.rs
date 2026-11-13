//! How a circuit is solved, separate from what it is: a topology names
//! its method as a type parameter or a constructor, never in its name.
//!
//! - [`dk`] — the nodal DK method: any MNA-solvable
//!   [`Netlist`](crate::circuit::netlist::Netlist), discretised into a
//!   state space with its nonlinear subsystem separated
//!   (`Circuit<Dk>`);
//! - [`newton`] — damped Newton with device step limiting, and the
//!   settings and statistics every iterative solve shares;
//! - [`load_line`] — a device on a supply through a load, in equilibrium
//!   every sample;
//! - [`wdf`] — wave digital filters: the element set, series, parallel
//!   and R-type adaptors, a netlist built into trees under an R-type root
//!   with a multi-port nonlinear root (`Circuit<Wdf>`), and the diode
//!   pair at a hand-built root ([`PairMethod`](wdf::PairMethod));
//! - [`wright_omega`] — closed forms of the Shockley equation in the wave
//!   domain.

pub mod dk;
pub mod load_line;
pub mod newton;
pub mod wdf;
pub mod wright_omega;
