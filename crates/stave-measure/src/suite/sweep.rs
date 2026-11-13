//! The exponential sine sweep (Farina, "Simultaneous measurement of
//! impulse response and distortion with a swept-sine technique", AES
//! 108th convention, 2000): one sweep, deconvolved by its inverse
//! filter, separates the linear impulse response from one response per
//! harmonic order, each arriving `L·ln n` before the linear one — the
//! fingerprint C12 fits Hammerstein kernels to.
//!
//! [`Sweep::synchronized`] is Novak et al.'s synchronized swept sine
//! ("Synchronized swept-sine: theory, application, and implementation",
//! JAES 63(10), 2015): the rate constant rounded so `f₁·L` is a whole
//! number, which makes every harmonic's response start in phase, so the
//! separated kernels' phases are right. Size it from the stage's memory
//! (≈0.6 s for order 5, ≈2 s for order 15 at a 20 ms response).

use std::f64::consts::TAU;

use crate::spectrum::fft;

/// `amplitude · sin(2π f₁ L (e^{t/L} − 1))` for `0 ≤ t < seconds`,
/// `L = seconds / ln(f₂/f₁)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Sweep {
    pub low_hz: f64,
    pub high_hz: f64,
    pub seconds: f64,
    pub amplitude: f64,
}

impl Sweep {
    pub fn exponential(low_hz: f64, high_hz: f64, seconds: f64, amplitude: f64) -> Self {
        assert!(0.0 < low_hz && low_hz < high_hz && seconds > 0.0);
        Self {
            low_hz,
            high_hz,
            seconds,
            amplitude,
        }
    }

    /// The synchronized sweep nearest `seconds` long: `L = round(f₁·T̂ /
    /// ln(f₂/f₁)) / f₁` (Novak's length rule), the length `L·ln(f₂/f₁)`.
    pub fn synchronized(low_hz: f64, high_hz: f64, seconds: f64, amplitude: f64) -> Self {
        let ratio = (high_hz / low_hz).ln();
        let l = (low_hz * seconds / ratio).round().max(1.0) / low_hz;
        Self::exponential(low_hz, high_hz, l * ratio, amplitude)
    }

    /// `L`: seconds per factor of e in frequency.
    pub fn rate_constant(&self) -> f64 {
        self.seconds / (self.high_hz / self.low_hz).ln()
    }

    pub fn at(&self, t: f64) -> f64 {
        if !(0.0..self.seconds).contains(&t) {
            return 0.0;
        }
        let l = self.rate_constant();
        self.amplitude * (TAU * self.low_hz * l * ((t / l).exp() - 1.0)).sin()
    }

    /// How far before the linear response harmonic `n`'s arrives.
    pub fn harmonic_advance(&self, n: u32) -> f64 {
        self.rate_constant() * f64::from(n).ln()
    }

    /// The sweep at `rate`.
    pub fn samples(&self, rate: f64) -> Vec<f64> {
        let n = (self.seconds * rate).round() as usize;
        (0..n).map(|i| self.at(i as f64 / rate)).collect()
    }

    /// The inverse filter: the sweep reversed, its envelope falling as
    /// `e^{−t/L}` (6 dB an octave, undoing the sweep's pink spectrum),
    /// scaled so the sweep through it peaks at one.
    pub fn inverse(&self, rate: f64) -> Vec<f64> {
        let x = self.samples(rate);
        let l = self.rate_constant();
        let mut inv: Vec<f64> = x
            .iter()
            .rev()
            .enumerate()
            .map(|(i, v)| v * (-(i as f64) / rate / l).exp())
            .collect();
        let peak = convolve(&x, &inv)
            .iter()
            .fold(0.0, |p: f64, v| p.max(v.abs()));
        inv.iter_mut().for_each(|v| *v /= peak);
        inv
    }

    /// `output` (the response to [`Sweep::samples`] at `rate`)
    /// deconvolved: the linear response peaks at index `samples − 1`, and
    /// harmonic `n`'s at [`Sweep::harmonic_advance`] before it.
    pub fn impulse_responses(&self, output: &[f64], rate: f64) -> Vec<f64> {
        convolve(output, &self.inverse(rate))
    }
}

/// Linear convolution by FFT.
fn convolve(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len() + b.len() - 1;
    let n = len.next_power_of_two();
    let (mut ar, mut ai) = (vec![0.0; n], vec![0.0; n]);
    let (mut br, mut bi) = (vec![0.0; n], vec![0.0; n]);
    ar[..a.len()].copy_from_slice(a);
    br[..b.len()].copy_from_slice(b);
    fft(&mut ar, &mut ai);
    fft(&mut br, &mut bi);
    // The product, conjugated: a forward FFT of the conjugate is the
    // inverse transform's conjugate, times n.
    let (mut pr, mut pi): (Vec<f64>, Vec<f64>) = (0..n)
        .map(|k| (ar[k] * br[k] - ai[k] * bi[k], -(ar[k] * bi[k] + ai[k] * br[k])))
        .unzip();
    fft(&mut pr, &mut pi);
    pr.truncate(len);
    pr.iter_mut().for_each(|v| *v /= n as f64);
    pr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wire_deconvolves_to_an_impulse_and_a_square_law_to_its_harmonic() {
        let rate = 48_000.0;
        let sweep = Sweep::synchronized(20.0, 20_000.0, 1.0, 1.0);
        let fl = sweep.low_hz * sweep.rate_constant();
        assert!((fl - fl.round()).abs() < 1e-9 && (sweep.seconds - 1.0).abs() < 0.1);
        let x = sweep.samples(rate);
        let wire = sweep.impulse_responses(&x, rate);
        let main = x.len() - 1;
        let (at, peak) = wire
            .iter()
            .enumerate()
            .fold((0, 0.0), |b, (i, &v)| if v.abs() > b.1 { (i, v.abs()) } else { b });
        assert_eq!(at, main);
        assert!((peak - 1.0).abs() < 1e-12);

        let bent: Vec<f64> = x.iter().map(|v| v + 0.1 * v * v).collect();
        let ir = sweep.impulse_responses(&bent, rate);
        let second = main - (sweep.harmonic_advance(2) * rate).round() as usize;
        let energy = |c: usize| (c - 64..c + 64).map(|i| ir[i] * ir[i]).sum::<f64>();
        let elsewhere = energy(second - 2_000);
        assert!(energy(second) > 100.0 * elsewhere, "{} vs {elsewhere}", energy(second));
    }
}
