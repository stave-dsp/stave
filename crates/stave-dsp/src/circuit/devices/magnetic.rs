//! The magnetic family: the [`Magnetic`] trait and its one model so far,
//! Jiles–Atherton.
//!
//! The Jiles–Atherton model of ferromagnetic hysteresis (Jiles &
//! Atherton 1986): magnetization M follows the anhysteretic curve
//! Man = Ms·L(He/a) of the effective field He = H + α·M, lagging it by
//! domain-wall pinning k, with a reversible fraction c. Driven by the
//! field H, the core advances M by the trapezoidal rule in H (Holters &
//! Zölzer, DAFx 2016), one Newton solve per step: the susceptibility's
//! slope in M analytic, the Langevin function and its two derivatives
//! from one exponential, the start's susceptibility carried over from
//! the last step, and the solve stopped on its quadratic convergence
//! (the next step predicted below tolerance) rather than one confirming
//! iteration later.

use crate::circuit::methods::newton::{NewtonSettings, NewtonStats, Solved};

/// Vacuum permeability, H/m.
pub const MU_0: f64 = 1.256_637_062_12e-6;

/// One core material's Jiles–Atherton parameters.
///
/// # Models
/// Jiles and Atherton's model of ferromagnetic hysteresis (1986):
/// saturation, anhysteretic shape, coupling, pinning and reversibility
/// of a core material.
///
/// # Derivation
/// The published model, advanced by the trapezoidal rule in H (Holters
/// & Zölzer, DAFx 2016), one Newton solve per step (in
/// `JilesAthertonCore`).
///
/// # Verified
/// `crates/stave-test/tests/matches_reference.rs::jiles_atherton_matches_reference`:
/// against an independent high-precision solution
/// (`scripts/ja-reference.py`), 5e-5 normalized RMS, converging with
/// rate, no capped solve;
/// `crates/stave-dsp/src/circuit/devices/magnetic.rs::the_loop_is_hysteretic_and_odd`.
///
/// # Cost
/// Per evaluation inside its circuit's line (`Transformer`); no
/// datasheet line yet (DSP.md P1).
///
/// # Aliasing
/// None of its own: a device; the circuit holding it runs oversampled.
///
/// # Valid ranges
/// Physical parameters (all positive, reversibility 0..1); fields
/// within a few times saturation.
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
        self.susceptibility_and_slope(m, h, rising).0
    }

    /// dM/dH on a branch and its partial derivative in M, `(χ, ∂χ/∂M)`:
    /// the Newton slope of the trapezoidal step, analytic.
    pub fn susceptibility_and_slope(&self, m: f64, h: f64, rising: bool) -> (f64, f64) {
        let (ms, a, alpha, c) = (self.saturation, self.shape, self.coupling, self.reversibility);
        let (l, l1, l2) = langevin_and_slopes((h + alpha * m) / a);
        let anhysteretic = ms * l;
        let anhysteretic_slope = ms / a * l1;
        // ∂/∂M of the anhysteretic slope (through He = H + αM).
        let anhysteretic_curvature = ms / a * l2 * alpha / a;
        let direction = if rising { 1.0 } else { -1.0 };
        let lag = anhysteretic - m;
        let lag_slope = alpha * anhysteretic_slope - 1.0;
        // Irreversible wall motion only toward the anhysteretic curve.
        let (irreversible, irreversible_slope) = if lag * direction > 0.0 {
            let pinned = (1.0 - c) * direction * self.pinning;
            let d = 1.0 / (pinned - alpha * lag);
            ((1.0 - c) * lag * d, (1.0 - c) * lag_slope * pinned * d * d)
        } else {
            (0.0, 0.0)
        };
        let numerator = irreversible + c * anhysteretic_slope;
        let numerator_slope = irreversible_slope + c * anhysteretic_curvature;
        let inverse = 1.0 / (1.0 - alpha * c * anhysteretic_slope);
        let chi = numerator * inverse;
        // (n/d)′ = (n′ + (n/d)·α·c·a″)/d, since d′ = −α·c·a″.
        let slope = (numerator_slope + chi * alpha * c * anhysteretic_curvature) * inverse;
        (chi, slope)
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

/// `(L, L′, L″)` at `x` (L(x) = coth x − 1/x, the Langevin function): by their series near 0, else from one
/// exponential `e = expm1(−2|x|)` — `coth|x| = −(2 + e)/e`,
/// `1/sinh²|x| = 4·(1 + e)/e²`, `L″ = 2·coth x/sinh² x − 2/x³` — with
/// `L` and `L″` odd and `L′` even.
#[inline]
fn langevin_and_slopes(x: f64) -> (f64, f64, f64) {
    if x.abs() < SERIES_BELOW {
        // Taylor series; the next terms (x⁷/4725, x⁶/675, 2x⁵/225's
        // successor) are below 1e-15 here.
        let x2 = x * x;
        (
            x * (1.0 / 3.0 - x2 * (1.0 / 45.0 - x2 * (2.0 / 945.0))),
            1.0 / 3.0 - x2 * (1.0 / 15.0 - x2 * (2.0 / 189.0)),
            x * (-2.0 / 15.0 + x2 * (8.0 / 189.0 - x2 * (2.0 / 225.0))),
        )
    } else {
        let t = x.abs();
        let e = (-2.0 * t).exp_m1();
        let inverse_e = 1.0 / e;
        let coth = -(2.0 + e) * inverse_e;
        let csch2 = 4.0 * (1.0 + e) * inverse_e * inverse_e;
        let inverse_t = 1.0 / t;
        let l = coth - inverse_t;
        let l2 = 2.0 * coth * csch2 - 2.0 * inverse_t * inverse_t * inverse_t;
        let sign = x.signum();
        (sign * l, inverse_t * inverse_t - csch2, sign * l2)
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
    /// χ at the committed point on the falling (`[0]`) and rising (`[1]`)
    /// branch, where the last step left it (NaN otherwise): a step the
    /// same way starts from it instead of evaluating it again.
    start: [f64; 2],
}

/// One trapezoidal step's solve, and χ at its end on its branch.
struct Step {
    solved: Solved,
    end_slope: f64,
}

impl JilesAthertonCore {
    pub fn new(model: JilesAtherton, newton: NewtonSettings) -> Self {
        Self {
            model,
            newton,
            field: 0.0,
            magnetization: 0.0,
            stats: NewtonStats::default(),
            start: [f64::NAN; 2],
        }
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// New material values, magnetic state kept.
    pub fn set_model(&mut self, model: JilesAtherton) {
        self.model = model;
        self.start = [f64::NAN; 2];
    }

    pub fn reset(&mut self) {
        self.field = 0.0;
        self.magnetization = 0.0;
        self.start = [f64::NAN; 2];
    }

    /// The committed magnetization M, A/m.
    pub fn magnetization(&self) -> f64 {
        self.magnetization
    }

    /// The committed field H, A/m.
    pub fn field(&self) -> f64 {
        self.field
    }

    /// Flux density B = μ0·(H + M), tesla.
    pub fn flux_density(&self) -> f64 {
        MU_0 * (self.field + self.magnetization)
    }

    /// Advance to field `h` (A/m); returns M (A/m).
    pub fn step(&mut self, h: f64) -> f64 {
        self.step_predicted(h, None)
    }

    /// Advance to field `h` (A/m), Newton started from `M₀ + χ̂·ΔH` with
    /// `χ̂` the caller's guess at the step's mean susceptibility ΔM/ΔH
    /// (`None`: χ at the start, the explicit Euler step). The solution
    /// does not depend on the guess, only the iterations do: a periodic
    /// drive (a tape's bias) guesses each step from the same step one
    /// period earlier.
    pub fn step_predicted(&mut self, h: f64, guess: Option<f64>) -> f64 {
        let dh = h - self.field;
        if dh != 0.0 {
            let rising = dh > 0.0;
            let step = self.solve(h, self.start_slope(rising), guess);
            self.stats.record_solve(&step.solved);
            self.start = [f64::NAN; 2];
            if step.solved.value.is_finite() {
                self.start[usize::from(rising)] = step.end_slope;
                self.magnetization = step.solved.value;
            }
        }
        self.field = h;
        self.magnetization
    }

    /// M at field `h` from the committed state, and the step's mean
    /// susceptibility ΔM/ΔH — without committing (an outer solve probes
    /// several fields per sample).
    pub fn trial(&self, h: f64) -> (f64, f64) {
        let dh = h - self.field;
        let rising = dh >= 0.0;
        let start = self.start_slope(rising);
        if dh == 0.0 {
            return (self.magnetization, start);
        }
        let solved = self.solve(h, start, None).solved;
        if solved.value.is_finite() {
            (solved.value, (solved.value - self.magnetization) / dh)
        } else {
            (self.magnetization, start)
        }
    }

    /// χ at the committed point on the rising (or falling) branch.
    #[inline]
    fn start_slope(&self, rising: bool) -> f64 {
        let carried = self.start[usize::from(rising)];
        if carried.is_nan() {
            self.model.susceptibility(self.magnetization, self.field, rising)
        } else {
            carried
        }
    }

    /// The trapezoidal step to `h` (≠ the committed field), `M − M₀ =
    /// ½·(χ(M, h) + χ₀)·ΔH` with `χ₀ = start`, by Newton on its analytic
    /// slope from the guess. A step that does not shrink the residual is
    /// halved, down to `min_damping`. Converged when a step is below
    /// tolerance, or when the quadratic convergence the last two show
    /// (`|δₖ| ≈ C·δₖ₋₁²`) puts the next below it: the step is taken and
    /// that prediction, `|δₖ|³/δₖ₋₁²`, is the reported final step — the
    /// iteration that would only confirm it is not run.
    #[inline]
    fn solve(&self, h: f64, start: f64, guess: Option<f64>) -> Step {
        let model = self.model;
        let newton = self.newton;
        let (m0, dh) = (self.magnetization, h - self.field);
        let rising = dh > 0.0;
        let half = 0.5 * dh;
        let residual = |m: f64| {
            let (chi, slope) = model.susceptibility_and_slope(m, h, rising);
            (m - m0 - half * (chi + start), chi, slope)
        };
        let mut m = m0 + guess.filter(|g| g.is_finite()).unwrap_or(start) * dh;
        let (mut r, mut chi, mut slope) = residual(m);
        let (mut iterations, mut previous, mut error) = (0, f64::INFINITY, f64::INFINITY);
        while iterations < newton.max_iterations {
            let step = r / (1.0 - half * slope);
            let size = step.abs();
            iterations += 1;
            error = if size < newton.tolerance {
                size
            } else if previous.is_finite() && size < previous {
                size * size * size / (previous * previous)
            } else {
                f64::INFINITY
            };
            if error < newton.tolerance {
                // χ at the end to first order in a step already below the
                // solve's resolution: its error is that step squared.
                m -= step;
                chi -= slope * step;
                break;
            }
            let mut damping = 1.0;
            loop {
                let candidate = m - damping * step;
                let (cr, cchi, cslope) = residual(candidate);
                if cr.abs() < r.abs() || damping <= newton.min_damping {
                    (m, r, chi, slope) = (candidate, cr, cchi, cslope);
                    break;
                }
                damping *= 0.5;
            }
            previous = damping * size;
        }
        Step {
            solved: Solved {
                value: m,
                iterations,
                converged: error < newton.tolerance,
                final_step: if error < newton.tolerance { error } else { previous },
            },
            end_slope: chi,
        }
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
        let curvature = |x: f64| -2.0 * x / 15.0 + 8.0 * x.powi(3) / 189.0 - 2.0 * x.powi(5) / 225.0;
        for x in [SERIES_BELOW * 0.99, SERIES_BELOW * 1.01, 1e-4, 1e-6] {
            for x in [x, -x] {
                let (l, l1, l2) = langevin_and_slopes(x);
                assert!((l - series(x)).abs() < 1e-13, "L({x})");
                assert!((l1 - slope(x)).abs() < 1e-10, "L'({x})");
                // L″'s closed form cancels as ε/x³: 1e-9 at the switch.
                assert!((l2 - curvature(x)).abs() < 1e-8, "L''({x})");
            }
        }
        // Away from the switch, against the textbook forms.
        for x in [0.05, 0.7, 3.0, -12.0, 300.0, 800.0] {
            let (l, l1, l2) = langevin_and_slopes(x);
            let (c, s) = (1.0 / x.tanh(), 1.0 / x.sinh());
            assert!((l - (c - 1.0 / x)).abs() < 1e-13, "L({x})");
            assert!((l1 - (1.0 / (x * x) - s * s)).abs() < 1e-12, "L'({x})");
            assert!((l2 - (2.0 * c * s * s - 2.0 / x.powi(3))).abs() < 1e-9, "L''({x})");
        }
    }

    #[test]
    fn the_newton_slope_is_the_susceptibilitys_derivative() {
        // ∂χ/∂M analytic against a central difference, on both branches,
        // either side of the anhysteretic curve, through the series switch.
        for (m, h) in [(5e4, 0.0), (2e5, 1_000.0), (-4e5, 3_000.0), (9e5, -800.0), (1e5, 20.0)] {
            for rising in [true, false] {
                let (_, slope) = CORE.susceptibility_and_slope(m, h, rising);
                let dm = 1e-3 * (1.0 + m.abs());
                let difference = (CORE.susceptibility(m + dm, h, rising) - CORE.susceptibility(m - dm, h, rising)) / (2.0 * dm);
                assert!(
                    (slope - difference).abs() <= 1e-6 * difference.abs() + 1e-15,
                    "({m}, {h}, {rising}): {slope} vs {difference}"
                );
            }
        }
    }

    /// The trapezoidal step solved as it was before the analytic slope:
    /// damped Newton on a difference quotient, to 1e-9 A/m, χ₀ evaluated.
    fn reference_step(model: JilesAtherton, m0: f64, h0: f64, h: f64) -> f64 {
        let dh = h - h0;
        if dh == 0.0 {
            return m0;
        }
        let rising = dh > 0.0;
        let start = model.susceptibility(m0, h0, rising);
        let residual = |m: f64| {
            let chi = model.susceptibility(m, h, rising);
            let probe = 1e-6 * (1.0 + m.abs());
            let ahead = model.susceptibility(m + probe, h, rising);
            (m - m0 - 0.5 * (chi + start) * dh, 1.0 - 0.5 * dh * (ahead - chi) / probe)
        };
        NewtonSettings::DEFAULT.solve_scalar(m0 + start * dh, residual, |new, _| new).value
    }

    #[test]
    fn the_fast_solve_is_the_trapezoidal_step() {
        // A tape's drive: a 14-sample bias at 2.5× the coercivity plus a
        // signal, through ferric tape; each step from the same committed
        // point both ways — carried start slope, periodic guess and
        // quadratic stop against the plain solve — and the loops kept
        // together over 50 bias cycles.
        let tape = JilesAtherton {
            saturation: 3.5e5,
            shape: 22e3,
            coupling: 1.6e-3,
            pinning: 27e3,
            reversibility: 0.17,
        };
        let mut core = JilesAthertonCore::new(tape, NewtonSettings::DEFAULT);
        let mut secants = [f64::NAN; 14];
        let mut worst: f64 = 0.0;
        for n in 0..700 {
            let signal = 13.5e3 * 1.5 * (std::f64::consts::TAU * n as f64 / 233.0).sin();
            let h = signal + 67.5e3 * (std::f64::consts::TAU * (n % 14) as f64 / 14.0).cos();
            let (m0, h0) = (core.magnetization(), core.field());
            let expected = reference_step(tape, m0, h0, h);
            let m = core.step_predicted(h, Some(secants[n % 14]));
            secants[n % 14] = (m - m0) / (h - h0);
            worst = worst.max((m - expected).abs());
        }
        // The start's χ, carried to first order against evaluated afresh,
        // differs by χ's rounding (L′ = 1/x² − 1/sinh² x cancels toward the
        // series switch: 1e-12 of χ on some platforms' libm), which ΔH of
        // tens of kA/m carries into M: 1e-6 A/m is 3e-12 of saturation.
        assert!(worst < 1e-6, "{worst} A/m from the plain solve");
        let stats = core.stats();
        assert_eq!(stats.capped, 0);
        assert!(stats.mean_iterations() < 2.5, "{}", stats.mean_iterations());
    }
}
