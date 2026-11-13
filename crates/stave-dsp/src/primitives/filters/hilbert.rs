//! The Hilbert transformer as a pair of IIR allpass chains whose outputs
//! stay 90° apart across the audio band: the analytic signal, for
//! frequency shifting and single-sideband modulation.

use crate::kernel::node::{Stage, Tail};
use crate::units::Hz;

/// The most allpass sections a design may use (both paths together):
/// enough for a 20 Hz band edge within 0.01° at 768 kHz.
pub const MAX_SECTIONS: usize = 40;

/// A Hilbert transformer: two allpass chains, `(re, im)` 90° apart
/// (`im` lagging) from `low` up to `fs/2 − low`, the phase difference
/// equiripple within the budget it was designed for.
///
/// # Models
/// The ideal Hilbert transform's phase, `−90°` at every positive
/// frequency, as the difference between two allpass paths: the analytic
/// signal `re + j·im` of the input, up to a common allpass phase (the
/// magnitude is exactly flat, the shared phase is not linear).
///
/// # Derivation
/// The polyphase IIR half-band (Valenzuela & Constantinides, *Digital
/// signal processing schemes for efficient interpolation and decimation*,
/// 1983), its allpass coefficients by the elliptic closed form (as in
/// Laurent de Soras's HIIR), shifted by a quarter of the rate
/// (`z⁻² → −z⁻²`) so the half-band's two paths become a 90° pair. The
/// transition `low/fs` and the phase budget choose the number of
/// sections at `prepare`, so the band edge is in hertz at every rate:
/// the stopband attenuation `A` of the half-band is the phase ripple
/// `2·asin(10^(−A/20))`. Sections `(a − z⁻²)/(1 − a·z⁻²)` in f64.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/filters/hilbert/tests.rs::quadrature_holds_across_the_band`:
/// measured on tones, the paths 90° ± 0.5° apart (the default budget)
/// and equal in level to 1e-6 dB, 20 Hz–20 kHz at 44.1, 48, 96 and
/// 192 kHz, within 0.02° of the design's own ripple bound.
///
/// # Cost
/// Datasheet `"Hilbert"` at 48 kHz (10 sections at the default design).
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// `low` from 1 Hz to a quarter of the rate; the budget from 0.001°.
/// A design that would need more than [`MAX_SECTIONS`] uses that many
/// and says so in [`phase_error_degrees`](Self::phase_error_degrees).
#[derive(Clone, Debug)]
pub struct Hilbert {
    low: Hz,
    budget_degrees: f64,
    sample_rate: f32,
    sections: usize,
    ripple_degrees: f64,
    coefficients: [f64; MAX_SECTIONS],
    /// Per section: x[n−1], x[n−2], y[n−1], y[n−2].
    state: [[f64; 4]; MAX_SECTIONS],
    /// The second path's one-sample delay.
    delayed: f64,
}

impl Default for Hilbert {
    /// 90° ± 0.5° from 20 Hz to `fs/2 − 20 Hz`.
    fn default() -> Self {
        Self::new(Hz::new(20.0), 0.5)
    }
}

impl Hilbert {
    /// A transformer whose paths stay `90° ± phase_error_degrees` apart
    /// from `low` to `fs/2 − low` (designed again at every `prepare`).
    pub fn new(low: Hz, phase_error_degrees: f64) -> Self {
        let mut h = Self {
            low,
            budget_degrees: phase_error_degrees,
            sample_rate: 48_000.0,
            sections: 0,
            ripple_degrees: 0.0,
            coefficients: [0.0; MAX_SECTIONS],
            state: [[0.0; 4]; MAX_SECTIONS],
            delayed: 0.0,
        };
        h.design();
        h
    }

    /// The design's equiripple phase error at the prepared rate, in
    /// degrees: within the budget unless [`MAX_SECTIONS`] capped it.
    pub fn phase_error_degrees(&self) -> f64 {
        self.ripple_degrees
    }

    /// Allpass sections in both paths together at the prepared rate.
    pub fn sections(&self) -> usize {
        self.sections
    }

    fn design(&mut self) {
        let fs = f64::from(self.sample_rate);
        let transition = (f64::from(self.low.get()) / fs).clamp(1.0 / fs, 0.25 - 1e-9);
        let budget = self.budget_degrees.max(1e-3);
        let (k, q) = transition_parameters(transition);
        let mut n = 1;
        while n < MAX_SECTIONS && ripple_degrees(n, q) > budget {
            n += 1;
        }
        self.sections = n;
        self.ripple_degrees = ripple_degrees(n, q);
        let order = 2 * n + 1;
        for (i, c) in self.coefficients.iter_mut().enumerate() {
            *c = if i < n {
                coefficient(i + 1, k, q, order)
            } else {
                0.0
            };
        }
    }

    /// One sample: `(re, im)`, `im` 90° behind `re`.
    #[inline]
    pub fn analytic(&mut self, x: f32) -> (f32, f32) {
        let (re, im) = self.analytic_f64(f64::from(x));
        (re as f32, im as f32)
    }

    /// [`analytic`](Self::analytic) in f64.
    #[inline]
    pub fn analytic_f64(&mut self, x: f64) -> (f64, f64) {
        let mut paths = [x, self.delayed];
        self.delayed = x;
        for i in 0..self.sections {
            let a = self.coefficients[i];
            let s = &mut self.state[i];
            let input = paths[i & 1];
            let y = a * (input + s[3]) - s[1];
            *s = [input, s[0], y, s[2]];
            paths[i & 1] = y;
        }
        (paths[0], paths[1])
    }
}

impl Stage for Hilbert {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.design();
        self.reset();
    }
    fn reset(&mut self) {
        self.state = [[0.0; 4]; MAX_SECTIONS];
        self.delayed = 0.0;
    }
    /// The in-phase path (an allpass); the pair is
    /// [`analytic`](Self::analytic).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.analytic(x).0
    }
    /// The slowest section's pole, `√a` for the largest `a`.
    fn tail(&self) -> Tail {
        let a = self.coefficients[..self.sections]
            .iter()
            .fold(0.0f64, |m, &a| m.max(a.abs()));
        Tail::from_pole(a.sqrt()).plus(1)
    }
}

/// The elliptic parameters of a half-band with transition `transition`
/// (relative to the rate): the selectivity `k` and the nome
/// `q = exp(−π·K(k′)/K(k))`, the complete elliptic integrals by the
/// arithmetic–geometric mean (exact where the usual four-term nome
/// series drifts: at a 20 Hz edge and 192 kHz it overstates the ripple's
/// accuracy by 0.06°).
fn transition_parameters(transition: f64) -> (f64, f64) {
    let k = ((1.0 - 2.0 * transition) * core::f64::consts::FRAC_PI_4)
        .tan()
        .powi(2);
    let complement = (1.0 - k * k).sqrt();
    let q = (-core::f64::consts::PI * agm(1.0, complement) / agm(1.0, k)).exp();
    (k, q)
}

/// The arithmetic–geometric mean, to f64 precision.
fn agm(mut a: f64, mut b: f64) -> f64 {
    for _ in 0..64 {
        if (a - b).abs() <= 1e-15 * a {
            break;
        }
        (a, b) = (0.5 * (a + b), (a * b).sqrt());
    }
    a
}

/// The phase ripple, in degrees, of `n` sections at nome `q`: the
/// half-band's stopband amplitude `√(a/(1+a))`, `a = 4·q^((2n+1)/2)`,
/// as `2·asin`.
fn ripple_degrees(n: usize, q: f64) -> f64 {
    let a = 4.0 * q.powf((2 * n + 1) as f64 * 0.5);
    let amplitude = (a / (1.0 + a)).sqrt().min(1.0);
    (2.0 * amplitude.asin()).to_degrees()
}

/// Section `c` (from 1) of an order-`order` design.
fn coefficient(c: usize, k: f64, q: f64, order: usize) -> f64 {
    let (c, order) = (c as f64, order as f64);
    let pi = core::f64::consts::PI;
    let mut num = 0.0;
    let mut i = 0i32;
    loop {
        let term = q.powi(i * (i + 1)) * ((2 * i + 1) as f64 * c * pi / order).sin();
        num += if i % 2 == 0 { term } else { -term };
        i += 1;
        if term.abs() < 1e-100 || i > 64 {
            break;
        }
    }
    let mut den = 0.0;
    let mut i = 1i32;
    loop {
        let term = q.powi(i * i) * ((2 * i) as f64 * c * pi / order).cos();
        den += if i % 2 == 0 { term } else { -term };
        i += 1;
        if term.abs() < 1e-100 || i > 64 {
            break;
        }
    }
    let ww = num * q.sqrt().sqrt() / (den + 0.5);
    let w2 = ww * ww;
    let x = ((1.0 - w2 * k) * (1.0 - w2 / k)).sqrt() / (1.0 + w2);
    (1.0 - x) / (1.0 + x)
}

#[cfg(test)]
mod tests;
