//! The minimum-phase half-band for [`Oversampled::minimum`](crate::Oversampled::minimum):
//! an odd-order elliptic half-band realised as two parallel cascades of
//! first-order allpasses in `z⁻²`,
//! `H(z) = ½·[A₀(z²) + z⁻¹·A₁(z²)]`, run polyphase so every section
//! works at the outer rate (Valenzuela & Constantinides, "Digital
//! signal processing schemes for efficient interpolation and
//! decimation", IEE Proc. G, 1983; the coefficient design in closed form
//! from the elliptic modulus, as in Laurent de Soras' HIIR).
//!
//! Its magnitude is power-complementary: the passband ripple is the
//! square of the stopband's (below 1e-9 dB at a 96 dB stopband), and its
//! group delay is a few samples at low frequencies, rising toward the
//! band edge, where a linear-phase design pays the full delay at every
//! frequency.

use core::f64::consts::PI;
use crate::units::Db;

/// The design's coefficients for a stopband of at least `attenuation_db`
/// and a transition `transition` wide on each side of a quarter of the
/// filter's rate (pass to `¼ − transition`, stop from `¼ + transition`),
/// ascending; even indices go to `A₀`, odd to `A₁`.
pub(crate) fn coefficients(attenuation_db: f64, transition: f64) -> Vec<f64> {
    // The elliptic modulus k and nome q of the transition.
    let mut k = ((1.0 - 2.0 * transition) * PI / 4.0).tan();
    k *= k;
    let kk = (1.0 - k * k).powf(0.25);
    let e = 0.5 * (1.0 - kk) / (1.0 + kk);
    let e4 = e.powi(4);
    let q = e * (1.0 + e4 * (2.0 + e4 * (15.0 + 150.0 * e4)));
    // The odd order that reaches the attenuation.
    let a2 = 10f64.powf(-attenuation_db / 10.0);
    let a = a2 / (1.0 - a2);
    let mut order = ((a * a / 16.0).ln() / q.ln()).ceil() as usize;
    if order % 2 == 0 {
        order += 1;
    }
    let order = order.max(3);
    (1..=(order - 1) / 2)
        .map(|c| {
            let (mut num, mut sign, mut i) = (0.0f64, 1.0f64, 0i32);
            loop {
                let t = q.powi(i * (i + 1))
                    * (((2 * i + 1) * c as i32) as f64 * PI / order as f64).sin()
                    * sign;
                num += t;
                sign = -sign;
                i += 1;
                if t.abs() <= 1e-100 {
                    break;
                }
            }
            let (mut den, mut sign, mut i) = (0.0f64, -1.0f64, 1i32);
            loop {
                let t =
                    q.powi(i * i) * ((2 * i * c as i32) as f64 * PI / order as f64).cos() * sign;
                den += t;
                sign = -sign;
                i += 1;
                if t.abs() <= 1e-100 {
                    break;
                }
            }
            let ww = num * q.powf(0.25) / (den + 0.5);
            let w2 = ww * ww;
            let x = ((1.0 - w2 * k) * (1.0 - w2 / k)).sqrt() / (1.0 + w2);
            (1.0 - x) / (1.0 + x)
        })
        .collect()
}

/// `0.0` for tiny or non-finite `x`: the recursions' denormal guard.
#[inline]
fn flush(x: f64) -> f64 {
    if (1e-30..=f64::MAX).contains(&x.abs()) {
        x
    } else {
        0.0
    }
}

/// One cascade of first-order allpasses `(a + z⁻¹)/(1 + a·z⁻¹)`.
#[derive(Clone)]
struct Cascade {
    a: Vec<f64>,
    /// `(x[n−1], y[n−1])` per section.
    state: Vec<(f64, f64)>,
}

impl Cascade {
    fn new(a: Vec<f64>) -> Self {
        let state = vec![(0.0, 0.0); a.len()];
        Self { a, state }
    }

    #[inline]
    fn tick(&mut self, mut x: f64) -> f64 {
        for (&a, (x1, y1)) in self.a.iter().zip(&mut self.state) {
            let y = flush(a * (x - *y1) + *x1);
            *x1 = x;
            *y1 = y;
            x = y;
        }
        x
    }

    fn reset(&mut self) {
        self.state.fill((0.0, 0.0));
    }

    /// Group delay at DC in samples: `(1 − a)/(1 + a)` per section.
    fn delay(&self) -> f64 {
        self.a.iter().map(|&a| (1.0 - a) / (1.0 + a)).sum()
    }
}

/// A 2× interpolator and decimator around one allpass half-band.
#[derive(Clone)]
pub(crate) struct Allpass {
    up: [Cascade; 2],
    down: [Cascade; 2],
    /// The decimator's odd path, one outer sample late (`z⁻¹·A₁`).
    held: f64,
}

impl Allpass {
    pub fn new(attenuation: Db, transition: f64) -> Self {
        let attenuation_db = f64::from(attenuation.get());
        let c = coefficients(attenuation_db, transition);
        let even: Vec<f64> = c.iter().copied().step_by(2).collect();
        let odd: Vec<f64> = c.iter().copied().skip(1).step_by(2).collect();
        let pair = || [Cascade::new(even.clone()), Cascade::new(odd.clone())];
        Self {
            up: pair(),
            down: pair(),
            held: 0.0,
        }
    }

    /// The round trip's group delay at DC, at this stage's outer rate:
    /// the half-band's own DC delay in inner samples,
    /// `½·(2·Σ_A₀ τ + 1 + 2·Σ_A₁ τ)`, twice (up and down) and halved
    /// back to the outer rate.
    pub fn delay(&self) -> f64 {
        0.5 * (2.0 * self.up[0].delay() + 1.0 + 2.0 * self.up[1].delay())
    }

    /// The slowest pole's radius per outer sample (each section's pole
    /// is at `−a` in `z`), for the tail estimate.
    pub fn slowest_pole(&self) -> f64 {
        self.up
            .iter()
            .flat_map(|c| c.a.iter())
            .fold(0.0f64, |m, &a| m.max(a.abs()))
    }

    pub fn reset(&mut self) {
        for c in self.up.iter_mut().chain(self.down.iter_mut()) {
            c.reset();
        }
        self.held = 0.0;
    }

    /// `input` (n) to `out` (2n): the even phase is `A₀`, the odd `A₁`.
    pub fn up(&mut self, input: &[f32], out: &mut [f32]) {
        for (&x, pair) in input.iter().zip(out.chunks_exact_mut(2)) {
            let x = f64::from(x);
            pair[0] = self.up[0].tick(x) as f32;
            pair[1] = self.up[1].tick(x) as f32;
        }
    }

    /// `input` (2n) to `out` (n): `½·(A₀·even + z⁻¹·A₁·odd)`.
    pub fn down(&mut self, input: &[f32], out: &mut [f32]) {
        for (y, pair) in out.iter_mut().zip(input.chunks_exact(2)) {
            let even = self.down[0].tick(f64::from(pair[0]));
            *y = (0.5 * (even + self.held)) as f32;
            self.held = self.down[1].tick(f64::from(pair[1]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `|H(e^{jω})|` of the design, from its two paths.
    fn gain(c: &[f64], f: f64) -> f64 {
        let w = core::f64::consts::TAU * f;
        // A first-order allpass in z⁻² at frequency w: phase only.
        let phase = |a: f64| {
            let (s, co) = (2.0 * w).sin_cos();
            // (a + e^{-2jw}) / (1 + a e^{-2jw})
            let num = (a + co, -s);
            let den = (1.0 + a * co, -a * s);
            num.1.atan2(num.0) - den.1.atan2(den.0)
        };
        let p0: f64 = c.iter().step_by(2).map(|&a| phase(a)).sum();
        let p1: f64 = c.iter().skip(1).step_by(2).map(|&a| phase(a)).sum::<f64>() - w;
        0.5 * ((p0.cos() + p1.cos()).powi(2) + (p0.sin() + p1.sin()).powi(2)).sqrt()
    }

    /// The front design (the 44.1 kHz edges at the 2× rate) reaches its
    /// stopband with ten sections and a passband flat to 1e-9 dB.
    #[test]
    fn the_front_design_meets_its_edges() {
        let t = 0.25 - 0.5 * super::super::polyphase::PASS;
        let c = coefficients(96.0, t);
        assert_eq!(c.len(), 10);
        let (mut pass, mut stop) = (0.0f64, f64::NEG_INFINITY);
        for i in 0..=2000 {
            let f = 0.5 * i as f64 / 2000.0;
            let db = 20.0 * gain(&c, f).log10();
            if f <= 0.25 - t {
                pass = pass.max(db.abs());
            }
            if f >= 0.25 + t {
                stop = stop.max(db);
            }
        }
        assert!(pass < 1e-9, "passband ripple {pass:e} dB");
        assert!(stop < -96.0, "stopband {stop:.1} dB");
    }
}
