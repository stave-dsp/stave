//! Antiderivative anti-aliasing (ADAA) — the stateful evaluator for any
//! [`AdaaShape`].
//!
//! First order (rectangular kernel — Parker/Zavalishin/Le Bivic 2016):
//! the difference quotient of the first antiderivative,
//! `y = (F1(x0) − F1(x1)) / (x0 − x1)`. Second order (triangular kernel —
//! Bilbao/Esqueda/Parker/Välimäki 2017): the twice-applied quotient of the
//! second antiderivative over (x0, x1, x2).
//!
//! Order guidance (measured on the triode tables): 1st order divides once,
//! so table interpolation error is amplified only ~1/Δx; 2nd order divides
//! twice (~1/Δx²) and can raise the *noise floor* audibly at low drive —
//! the oversampler covers the near-Nyquist band instead. Default to 1.
//!
//! Contract with the shape: `f1`/`f2` must be exact antiderivatives of
//! `f`, and the shape must be **frozen across one call** — if a bias or
//! drive frame moves per sample, commit the change *between* `process`
//! calls (the difference quotient mixes evaluations at x0 and x1, which
//! must see the same curve). All three ill-conditioned branches
//! (|x0−x1|, |x0−x2|, |x1−x2| → 0) fall back to lower-order antiderivative
//! or direct evaluation to avoid 0/0.

use crate::behavioral::curve::AdaaShape;

/// Below this input delta the difference quotient is ill-conditioned and
/// the evaluator falls back a rank (F1-quotient → midpoint f, etc.).
const EPS: f64 = 1.0e-5;

/// Kernel order; see the module header for why `First` is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdaaOrder {
    #[default]
    First,
    Second,
}

/// ADAA state: the previous two inputs + the selected order.
#[derive(Clone, Copy, Debug)]
pub struct Adaa {
    x1: f64,
    x2: f64,
    order: AdaaOrder,
}

impl Default for Adaa {
    fn default() -> Self {
        Self::new(AdaaOrder::First)
    }
}

impl Adaa {
    pub fn new(order: AdaaOrder) -> Self {
        Self {
            x1: 0.0,
            x2: 0.0,
            order,
        }
    }

    pub fn set_order(&mut self, order: AdaaOrder) {
        self.order = order;
    }

    pub fn order(&self) -> AdaaOrder {
        self.order
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
    }

    /// Anti-aliased evaluation of `shape` at `x0`; advances state.
    #[inline]
    pub fn process(&mut self, shape: &impl AdaaShape, x0: f64) -> f64 {
        let y = match self.order {
            AdaaOrder::First => self.adaa1(shape, x0),
            AdaaOrder::Second => self.adaa2(shape, x0),
        };
        self.advance(x0);
        y
    }

    /// Advance the input history without evaluating — used by callers that
    /// evaluated the shape directly (ADAA bypassed for A/B) so a later
    /// re-enable starts from consistent state.
    #[inline]
    pub fn advance(&mut self, x0: f64) {
        self.x2 = self.x1;
        self.x1 = x0;
    }

    #[inline]
    fn adaa1(&self, s: &impl AdaaShape, x0: f64) -> f64 {
        let x1 = self.x1;
        if (x0 - x1).abs() > EPS {
            (s.f1(x0) - s.f1(x1)) / (x0 - x1)
        } else {
            s.f(0.5 * (x0 + x1))
        }
    }

    #[inline]
    fn adaa2(&self, s: &impl AdaaShape, x0: f64) -> f64 {
        let x1 = self.x1;
        let x2 = self.x2;
        let outer = x0 - x2;
        if outer.abs() > EPS {
            (2.0 / outer) * (calc_d2(s, x0, x1) - calc_d2(s, x1, x2))
        } else {
            // x0 ≈ x2: L'Hôpital fallback around the midpoint.
            let xbar = 0.5 * (x0 + x2);
            let d = xbar - x1;
            if d.abs() > EPS {
                (2.0 / d) * (s.f1(xbar) + (s.f2(x1) - s.f2(xbar)) / d)
            } else {
                // all three nearly equal → direct transfer at the point.
                s.f(0.5 * (xbar + x1))
            }
        }
    }
}

/// First-order difference quotient of F2 (or its midpoint fallback).
#[inline]
fn calc_d2(s: &impl AdaaShape, a: f64, b: f64) -> f64 {
    if (a - b).abs() > EPS {
        (s.f2(a) - s.f2(b)) / (a - b)
    } else {
        s.f1(0.5 * (a + b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavioral::curve::Curve;

    fn tanh_curve() -> Curve {
        let mut c = Curve::flat();
        c.rebuild(-8.0, 8.0, 0.0, |x| x.tanh());
        c
    }

    fn goertzel(buf: &[f64], f: f64, fs: f64) -> f64 {
        let n = buf.len();
        let w = 2.0 * core::f64::consts::PI * f / fs;
        let cw = w.cos();
        let coeff = 2.0 * cw;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let re = s1 - s2 * cw;
        let im = s2 * w.sin();
        (re * re + im * im).sqrt() / (n as f64 * 0.5)
    }

    #[test]
    fn dc_and_slow_signals_reproduce_the_transfer() {
        // With no HF to band-limit, ADAA must converge to f(x) exactly.
        let c = tanh_curve();
        let fr = c.framed(0.0, 3.0, 1.0);
        for order in [AdaaOrder::First, AdaaOrder::Second] {
            let mut a = Adaa::new(order);
            for &dc in &[-0.7f64, -0.1, 0.25, 0.9] {
                for _ in 0..4 {
                    let _ = a.process(&fr, dc);
                }
                let y = a.process(&fr, dc);
                let f = (3.0 * dc).tanh();
                assert!(
                    (y - f).abs() < 1e-5,
                    "order {order:?} DC mismatch at {dc}: {y} vs {f}"
                );
            }
        }
    }

    #[test]
    fn ill_conditioned_branches_are_finite() {
        let c = tanh_curve();
        let fr = c.framed(0.0, 4.0, 1.0);
        for order in [AdaaOrder::First, AdaaOrder::Second] {
            let mut a = Adaa::new(order);
            // constant, period-2 alternation (x0≈x2, x1 differs), and
            // sub-epsilon deltas — all must stay finite and bounded.
            for i in 0..64 {
                let x = if i % 2 == 0 { 0.5 } else { -0.5 };
                let y = a.process(&fr, x);
                assert!(
                    y.is_finite() && y.abs() < 4.0,
                    "order {order:?} period-2 blew up"
                );
            }
            for _ in 0..8 {
                let y = a.process(&fr, 0.31);
                assert!(y.is_finite());
            }
            let y = a.process(&fr, 0.31 + 1e-9);
            assert!(y.is_finite() && (y - (4.0 * 0.31f64).tanh()).abs() < 1e-3);
        }
    }

    #[test]
    fn adaa_reduces_aliasing_on_a_hard_driven_tone() {
        // 5 kHz sine into tanh(6x) at 48 kHz: the 9th harmonic (45 kHz)
        // folds to 3 kHz. ADAA must hold that alias bin well below the
        // direct-evaluation baseline. (Both paths share the same table, so
        // this isolates the ADAA machinery.)
        let c = tanh_curve();
        let fr = c.framed(0.0, 6.0, 1.0);
        let fs = 48_000.0;
        let f0 = 5_000.0;
        let n = 16_384;
        let render = |use_adaa: bool| -> Vec<f64> {
            let mut a = Adaa::new(AdaaOrder::First);
            let mut out = vec![0.0f64; n];
            for i in 0..2 * n {
                let x = 0.9 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin();
                let y = if use_adaa {
                    a.process(&fr, x)
                } else {
                    a.advance(x);
                    fr.f(x)
                };
                if i >= n {
                    out[i - n] = y;
                }
            }
            out
        };
        let raw = goertzel(&render(false), 3_000.0, fs);
        let adaa = goertzel(&render(true), 3_000.0, fs);
        let gain_db = 20.0 * (raw / adaa.max(1e-12)).log10();
        println!("alias @3 kHz: raw {raw:.2e}, ADAA {adaa:.2e} ({gain_db:.1} dB better)");
        assert!(
            gain_db > 6.0,
            "ADAA should cut the folded harmonic: {gain_db} dB"
        );
    }
}
