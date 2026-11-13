//! The magnetic family: the [`Magnetic`] trait and its one model so far,
//! Jiles–Atherton.
//!
//! The Jiles–Atherton model of ferromagnetic hysteresis (Jiles &
//! Atherton 1986): magnetization M follows the anhysteretic curve
//! Man = Ms·L(He/a) of the effective field He = H + α·M, lagging it by
//! domain-wall pinning k, with a reversible fraction c. Driven by the
//! field H, the core advances M by the trapezoidal rule in H (Holters &
//! Zölzer, DAFx 2016), one Newton solve per step.

use crate::circuit::methods::newton::{NewtonSettings, NewtonStats, Solved};

/// Vacuum permeability, H/m.
pub const MU_0: f64 = 1.256_637_062_12e-6;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct JilesAtherton {
    /// Saturation magnetization Ms, A/m.
    pub saturation: f64,
    /// Anhysteretic shape a, A/m.
    pub shape: f64,
    /// Inter-domain coupling α.
    pub coupling: f64,
    /// Domain-wall pinning k, A/m.
    pub pinning: f64,
    /// Reversible fraction c, 0..1.
    pub reversibility: f64,
}

impl JilesAtherton {
    /// dM/dH on a branch rising (`rising`) or falling through H.
    pub fn susceptibility(&self, m: f64, h: f64, rising: bool) -> f64 {
        let x = (h + self.coupling * m) / self.shape;
        let anhysteretic = self.saturation * langevin(x);
        let anhysteretic_slope = self.saturation / self.shape * langevin_slope(x);
        let direction = if rising { 1.0 } else { -1.0 };
        let lag = anhysteretic - m;
        // Irreversible wall motion only toward the anhysteretic curve.
        let irreversible = if lag * direction > 0.0 {
            (1.0 - self.reversibility) * lag
                / ((1.0 - self.reversibility) * direction * self.pinning - self.coupling * lag)
        } else {
            0.0
        };
        (irreversible + self.reversibility * anhysteretic_slope)
            / (1.0 - self.coupling * self.reversibility * anhysteretic_slope)
    }
}

/// The magnetic family: a hysteretic material's differential
/// susceptibility dM/dH on a rising or falling branch, at magnetization
/// `m` and field `h` (A/m). Every core model implements it.
pub trait Magnetic: Copy {
    fn susceptibility(&self, m: f64, h: f64, rising: bool) -> f64;
}

impl Magnetic for JilesAtherton {
    #[inline]
    fn susceptibility(&self, m: f64, h: f64, rising: bool) -> f64 {
        JilesAtherton::susceptibility(self, m, h, rising)
    }
}

/// L(x) = coth x − 1/x, by its series near 0.
fn langevin(x: f64) -> f64 {
    if x.abs() < SERIES_BELOW {
        // Taylor series; the next term (x⁷/4725) is below 1e-18 here.
        let x2 = x * x;
        x * (1.0 / 3.0 - x2 * (1.0 / 45.0 - x2 * (2.0 / 945.0)))
    } else {
        1.0 / x.tanh() - 1.0 / x
    }
}

/// L′(x) = 1/x² − 1/sinh² x.
fn langevin_slope(x: f64) -> f64 {
    if x.abs() < SERIES_BELOW {
        // Taylor series; the next term (x⁶/675) is below 1e-15 here.
        let x2 = x * x;
        1.0 / 3.0 - x2 * (1.0 / 15.0 - x2 * (2.0 / 189.0))
    } else {
        1.0 / (x * x) - 1.0 / x.sinh().powi(2)
    }
}

/// Below this |x| the closed forms cancel catastrophically (their
/// rounding grows as ε/x and ε/x²: 2e-12 at 1e-4, past the series'
/// accuracy, and the size of the error depends on the platform's libm);
/// the series take over.
const SERIES_BELOW: f64 = 1e-2;

/// A core driven by its field: `step(h)` returns the magnetization.
#[derive(Clone)]
pub struct JilesAthertonCore {
    model: JilesAtherton,
    newton: NewtonSettings,
    field: f64,
    magnetization: f64,
    stats: NewtonStats,
}

impl JilesAthertonCore {
    pub fn new(model: JilesAtherton, newton: NewtonSettings) -> Self {
        Self {
            model,
            newton,
            field: 0.0,
            magnetization: 0.0,
            stats: NewtonStats::default(),
        }
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// New material values, magnetic state kept.
    pub fn set_model(&mut self, model: JilesAtherton) {
        self.model = model;
    }

    pub fn reset(&mut self) {
        self.field = 0.0;
        self.magnetization = 0.0;
    }

    /// Flux density B = μ0·(H + M), tesla.
    pub fn flux_density(&self) -> f64 {
        MU_0 * (self.field + self.magnetization)
    }

    /// Advance to field `h` (A/m); returns M (A/m).
    pub fn step(&mut self, h: f64) -> f64 {
        let m = match self.solve(h) {
            (Some(solved), _) => {
                self.stats.record_solve(&solved);
                solved.value
            }
            (None, _) => self.magnetization,
        };
        self.field = h;
        self.magnetization = m;
        m
    }

    /// M at field `h` from the committed state, and the step's mean
    /// susceptibility ΔM/ΔH — without committing (an outer solve probes
    /// several fields per sample).
    pub fn trial(&self, h: f64) -> (f64, f64) {
        match self.solve(h) {
            (Some(solved), _) => (
                solved.value,
                (solved.value - self.magnetization) / (h - self.field),
            ),
            (None, chi) => (self.magnetization, chi),
        }
    }

    /// The trapezoidal step to `h`; `None` when the field has not moved
    /// (then the susceptibility at the committed point).
    fn solve(&self, h: f64) -> (Option<Solved>, f64) {
        let model = self.model;
        let dh = h - self.field;
        if dh == 0.0 {
            return (None, model.susceptibility(self.magnetization, h, true));
        }
        let rising = dh > 0.0;
        let (m0, h0) = (self.magnetization, self.field);
        let start_slope = model.susceptibility(m0, h0, rising);
        // M − M₀ = ½·(χ(M, h) + χ(M₀, h₀))·Δh
        let residual = |m: f64| {
            let chi = model.susceptibility(m, h, rising);
            let r = m - m0 - 0.5 * (chi + start_slope) * dh;
            let probe = 1e-6 * (1.0 + m.abs());
            let chi_probe = model.susceptibility(m + probe, h, rising);
            (r, 1.0 - 0.5 * dh * (chi_probe - chi) / probe)
        };
        let solved = self
            .newton
            .solve_scalar(m0 + start_slope * dh, residual, |new, _| new);
        (Some(solved), start_slope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jiles & Atherton's 1986 steel: α·Ms/(3a) = 0.78 < 1, so the
    /// coupling feedback stays stable.
    const CORE: JilesAtherton = JilesAtherton {
        saturation: 1.6e6,
        shape: 1_100.0,
        coupling: 1.6e-3,
        pinning: 400.0,
        reversibility: 0.2,
    };

    fn cycle(core: &mut JilesAthertonCore, peak: f64, n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let h = peak * (std::f64::consts::TAU * i as f64 / n as f64).sin();
                (h, core.step(h))
            })
            .collect()
    }

    #[test]
    fn the_loop_is_hysteretic_and_odd() {
        let mut core = JilesAthertonCore::new(CORE, NewtonSettings::DEFAULT);
        cycle(&mut core, 5_000.0, 4_000);
        let settled = cycle(&mut core, 5_000.0, 4_000);
        // Remanence: crossing H = 0 rising and falling, M differs in sign.
        let falling = settled[2_000].1;
        assert!(
            falling > 0.0,
            "remanence after the positive peak: {falling}"
        );
        let at = |i: usize| settled[i].1;
        assert!((at(1_000) + at(3_000)).abs() < 1e-3 * at(1_000).abs());
        assert_eq!(core.stats().capped, 0);
    }

    #[test]
    fn langevin_is_continuous_at_its_series_switch() {
        // Either side of the switch, both forms agree with the series to
        // well inside any platform's libm rounding (glibc, the CRT and
        // Apple's libm all hold this).
        let series = |x: f64| x / 3.0 - x.powi(3) / 45.0 + 2.0 * x.powi(5) / 945.0;
        let slope = |x: f64| 1.0 / 3.0 - x * x / 15.0 + 2.0 * x.powi(4) / 189.0;
        for x in [SERIES_BELOW * 0.99, SERIES_BELOW * 1.01, 1e-4, 1e-6] {
            assert!((langevin(x) - series(x)).abs() < 1e-13, "L({x})");
            assert!((langevin_slope(x) - slope(x)).abs() < 1e-10, "L'({x})");
        }
    }
}
