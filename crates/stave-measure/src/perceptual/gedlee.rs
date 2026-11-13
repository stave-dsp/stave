//! The GedLee metric `G_m` of a static transfer curve (Geddes & Lee
//! 2003, as De Santis 2007 restates it):
//!
//! ```text
//! G_m = sqrt( ∫₋₁¹ cos²(πx/2) · T''(x)² dx )
//! ```
//!
//! # Models
//!
//! Two masking effects on a memoryless curve `T` over the input range
//! normalised to ±1: the second derivative weights higher orders more
//! (an order-`k` term grows `k(k−1)` times in `T''`, as higher harmonics
//! escape the masking of lower ones) and ignores gain and offset; the
//! `cos²` weights small inputs more (low levels mask less). A linear
//! curve is 0.
//!
//! A circuit with memory has no static curve; [`GedLee::of_harmonics`]
//! takes the curve a tone's harmonics describe instead — for a
//! memoryless `T` driven by `A·cos θ`, the output's harmonics are the
//! Chebyshev coefficients of `T(A·u)`, so `T(u) = Σ a_h·T_h(u)` with
//! `a_h` each harmonic's amplitude in phase with the fundamental
//! (`Re(c_h·e^{−jh·arg c_1})`). The curve is divided by the
//! fundamental's amplitude: unit gain at that level, so the figure is
//! the curve's shape, not its gain.
//!
//! # Derivation
//!
//! The formula as printed by De Santis 2007 (eq. 2.15). De Santis's
//! worked value for `T = x + x³`, 1.5, is not what the printed formula
//! gives (`sqrt(36·(1/3 − 2/π²))` = 2.169); this implementation follows
//! the formula, and its test holds the closed form.
//!
//! # Verified
//!
//! `crates/stave-measure/src/perceptual/gedlee.rs::tests`: the closed
//! form for `x + x³`; zero for a gain and an offset; at equal THD,
//! higher orders read higher; the harmonic route agrees with the curve
//! for a memoryless polynomial.
//!
//! # Cost
//!
//! `"GedLee (15 harmonics)"`: a few thousand curve evaluations per
//! figure, once per tone run.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Smooth curves (a hard clip's kink has an unbounded `T''`; the
//! harmonic route reads its band-limited version, to the harmonics it
//! has).

use stave_dsp::primitives::spectral::Complex;

pub const VERSION: u32 = 1;
/// Simpson panels over [−1, 1].
const PANELS: usize = 2_000;

/// A GedLee figure.
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GedLee {
    pub gm: f64,
    pub version: u32,
}

impl GedLee {
    /// `G_m` of `t` over [−1, 1], `t''` by central differences.
    pub fn of_curve(t: impl Fn(f64) -> f64) -> Self {
        let h = 1e-4;
        Self::integrate(|x| (t(x + h) - 2.0 * t(x) + t(x - h)) / (h * h))
    }

    /// `G_m` of the curve a tone's harmonics describe: `harmonics[h − 1]`
    /// is harmonic `h`'s complex amplitude (any phase convention whose
    /// delay rotates harmonic `h` by `h` times the fundamental's), the
    /// curve divided by the fundamental's amplitude.
    pub fn of_harmonics(harmonics: &[Complex<f64>]) -> Self {
        let c1 = harmonics[0];
        let (g, phi) = (c1.norm(), c1.arg());
        let a: Vec<f64> = harmonics
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let h = (i + 1) as f64;
                (c.re * (h * phi).cos() + c.im * (h * phi).sin()) / g
            })
            .collect();
        // T''(u) = Σ a_h·T_h''(u), by the Chebyshev recurrences for
        // T_h, T_h' and T_h''.
        Self::integrate(move |u| {
            let (mut t0, mut t1) = (1.0, u);
            let (mut d0, mut d1) = (0.0, 1.0);
            let (mut s0, mut s1) = (0.0, 0.0);
            let mut sum = 0.0;
            for (i, ah) in a.iter().enumerate() {
                if i > 0 {
                    // T_{h+1} = 2u·T_h − T_{h−1}; differentiate twice.
                    let t2 = 2.0 * u * t1 - t0;
                    let d2 = 2.0 * t1 + 2.0 * u * d1 - d0;
                    let s2 = 4.0 * d1 + 2.0 * u * s1 - s0;
                    (t0, t1, d0, d1, s0, s1) = (t1, t2, d1, d2, s1, s2);
                }
                sum += ah * s1;
            }
            sum
        })
    }

    fn integrate(second: impl Fn(f64) -> f64) -> Self {
        let h = 2.0 / PANELS as f64;
        let f = |x: f64| {
            let c = (std::f64::consts::FRAC_PI_2 * x).cos();
            c * c * second(x).powi(2)
        };
        let mut sum = f(-1.0) + f(1.0);
        for i in 1..PANELS {
            let x = -1.0 + i as f64 * h;
            sum += if i % 2 == 1 { 4.0 } else { 2.0 } * f(x);
        }
        Self {
            gm: (sum * h / 3.0).max(0.0).sqrt(),
            version: VERSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn the_closed_form_for_x_plus_x_cubed() {
        let want = (36.0 * (1.0 / 3.0 - 2.0 / (PI * PI))).sqrt();
        let got = GedLee::of_curve(|x| x + x * x * x).gm;
        assert!((got - want).abs() < 1e-5, "{got} vs {want}");
    }

    #[test]
    fn gain_and_offset_read_zero() {
        assert!(GedLee::of_curve(|x| 3.0 * x + 0.2).gm < 1e-6);
    }

    /// Each pure order `x + ε·T_k(x)` at the same THD (the same
    /// harmonic amplitude ε): higher `k`, higher `G_m`.
    #[test]
    fn higher_orders_weigh_more_at_equal_thd() {
        let eps = 0.01;
        let cheb = |k: i32, x: f64| (f64::from(k) * x.clamp(-1.0, 1.0).acos()).cos();
        let gm: Vec<f64> = (2..=7).map(|k| GedLee::of_curve(|x| x + eps * cheb(k, x)).gm).collect();
        assert!(gm.windows(2).all(|w| w[1] > w[0]), "{gm:?}");
    }

    /// A tone's harmonics through x + 0.3x² + 0.1x³ − 0.05x⁵ give back the
    /// curve (divided by the fundamental), under a delay too.
    #[test]
    fn harmonics_describe_the_curve() {
        let t = |x: f64| x + 0.3 * x * x + 0.1 * x.powi(3) - 0.05 * x.powi(5);
        // y(θ) = T(cos θ) by DFT: c_h = (2/N)·Σ y·e^{−jhθ}.
        let n = 64;
        let delay = 0.3; // radians of the fundamental
        let c: Vec<Complex<f64>> = (1..=15)
            .map(|h| {
                let (mut re, mut im) = (0.0, 0.0);
                for i in 0..n {
                    let th = std::f64::consts::TAU * i as f64 / n as f64;
                    let y = t((th - delay).cos());
                    re += y * (h as f64 * th).cos();
                    im -= y * (h as f64 * th).sin();
                }
                Complex::new(2.0 * re / n as f64, 2.0 * im / n as f64)
            })
            .collect();
        let g = c[0].norm();
        let curve = GedLee::of_curve(|x| t(x) / g).gm;
        let read = GedLee::of_harmonics(&c).gm;
        assert!((curve - read).abs() < 1e-4 * curve, "{curve} vs {read}");
    }
}
