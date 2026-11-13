//! The player's valve at the mouthpiece: a reed or the lips, as a
//! quasi-static flow law solved against the bore's waves.

use crate::units::Hz;

/// What the player blows through.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Valve {
    /// A beating reed (clarinet, saxophone): inward-striking and
    /// massless, its opening `(1 − Δp/p_M)` closing as the pressure
    /// across it reaches the closing pressure `p_M`.
    Reed,
    /// The lips (brass): outward-striking, a mass on a spring resonant
    /// at `frequency` with quality `q`, opened by the pressure across
    /// them.
    Lip { frequency: Hz, q: f64 },
}

/// The flow through the valve's `opening` for the pressure `delta`
/// across it (both dimensionless: pressure over `p_M`), Bernoulli's
/// `ζ · opening · sgn Δ √|Δ|`.
#[inline]
pub(super) fn flow(embouchure: f64, opening: f64, delta: f64) -> f64 {
    embouchure * opening.max(0.0) * delta.signum() * delta.abs().sqrt()
}

/// Solve the mouthpiece: pressure `p = q + h·u` where the bore's waves
/// give `q` and `h`, the flow `u` is the valve's at `Δ = γ − p`. In
/// `s = sgn Δ √|Δ|` the residual is monotone (for `hζ < 1`), so a
/// bracketed Newton converges. A reed's opening is `(1 − Δ)⁺` (a cubic
/// in `s`); a lip's is the given `opening` (a quadratic: closed form).
/// Returns `(Δ, u, iterations)`.
#[inline]
pub(super) fn solve(reed: bool, embouchure: f64, opening: f64, blow: f64, q: f64, h: f64) -> (f64, f64, u32) {
    let r = blow - q;
    let k = h * embouchure;
    if !reed {
        let k = k * opening.max(0.0);
        let s = if r >= 0.0 { 0.5 * (-k + (k * k + 4.0 * r).sqrt()) } else { 0.5 * (k - (k * k - 4.0 * r).sqrt()) };
        let delta = s * s.abs();
        return (delta, flow(embouchure, opening, delta), 0);
    }
    if r >= 1.0 {
        // Beating: the reed is shut.
        return (r, 0.0, 0);
    }
    // f(s) = s|s| + k (1 − s|s|) s − r, increasing; bracket it.
    let f = |s: f64| {
        let d = s * s.abs();
        (d + k * (1.0 - d) * s - r, 2.0 * s.abs() + k * (1.0 - 3.0 * d))
    };
    let (mut lo, mut hi) = if r >= 0.0 { (0.0, 1.0) } else { (-(-r).sqrt(), 0.0) };
    let mut s = if r >= 0.0 { r.sqrt().min(1.0) * 0.5 } else { 0.5 * lo };
    let mut iterations = 0;
    for _ in 0..40 {
        iterations += 1;
        let (v, slope) = f(s);
        if v < 0.0 {
            lo = s;
        } else {
            hi = s;
        }
        let newton = if slope > 0.0 { s - v / slope } else { f64::NAN };
        let next = if newton > lo && newton < hi { newton } else { 0.5 * (lo + hi) };
        let done = (next - s).abs() <= 1e-13;
        s = next;
        if done {
            break;
        }
    }
    let delta = s * s.abs();
    (delta, flow(embouchure, 1.0 - delta, delta), iterations)
}
