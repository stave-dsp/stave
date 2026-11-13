//! An antiparallel diode pair at a tree's root, and the methods that
//! solve it: each takes the wave arriving through the root's port and
//! returns the wave the pair reflects.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::circuit::methods::wright_omega::WrightOmega;

/// How an antiparallel Shockley pair at a wave-digital root is solved:
/// the method a [`DiodeClipper`](crate::circuit::topologies::diode_clipper::DiodeClipper)
/// takes as its type parameter.
pub trait PairMethod: Copy {
    /// The wave the pair reflects for an arriving `wave` through port
    /// resistance `ohms`. `volts` holds the pair's voltage from the last
    /// sample on entry (a warm start) and this sample's on return; an
    /// iterative method records its solve in `stats`.
    fn reflect(
        &self,
        diode: &Shockley,
        wave: f64,
        ohms: f64,
        volts: &mut f64,
        stats: &mut NewtonStats,
    ) -> f64;
}

/// Newton on the exact pair, v + R·i(v) = a, with SPICE's junction
/// limiting.
impl PairMethod for NewtonSettings {
    #[inline]
    fn reflect(
        &self,
        diode: &Shockley,
        wave: f64,
        ohms: f64,
        volts: &mut f64,
        stats: &mut NewtonStats,
    ) -> f64 {
        let diode = *diode;
        let residual = |v: f64| {
            let (i, g) = diode.pair_current_and_conductance(v);
            (v + ohms * i - wave, 1.0 + ohms * g)
        };
        let solved = self.solve_scalar(*volts, residual, |new, old| {
            diode.limit_pair_step(new, old)
        });
        stats.record_solve(&solved);
        *volts = solved.value;
        2.0 * *volts - wave
    }
}

/// The closed form: no iterations, nothing recorded.
impl PairMethod for WrightOmega {
    #[inline]
    fn reflect(
        &self,
        diode: &Shockley,
        wave: f64,
        ohms: f64,
        volts: &mut f64,
        _: &mut NewtonStats,
    ) -> f64 {
        let b = WrightOmega::reflect_pair(diode, wave, ohms);
        *volts = 0.5 * (wave + b);
        b
    }
}
