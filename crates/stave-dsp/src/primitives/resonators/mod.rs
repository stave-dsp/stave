//! Tuned resonators: the pitched [`Comb`], the [`KarplusStrong`]
//! string, and the modal [`Bank`]. Each is tuned in [`Hz`](crate::Hz)
//! and decays by a T60 in [`Seconds`](crate::Seconds); the delay-based
//! ones take S2's fractional-delay [`Interpolator`] as a type parameter,
//! so their tuning is as exact as the read they pay for.
//!
//! [`Interpolator`]: crate::primitives::time::interpolators::Interpolator

use crate::primitives::time::interpolators::Interpolator;

mod bank;
mod comb;
mod karplus_strong;

pub use bank::{Bank, Mode};
pub use comb::Comb;
pub use karplus_strong::KarplusStrong;

/// The per-sample loop gain that decays 60 dB in `t60` seconds when it
/// is applied once every `period` seconds: `10^(−3·period/t60)`.
#[inline]
fn loop_gain(period: f64, t60: f64) -> f64 {
    if t60 <= 0.0 {
        0.0
    } else {
        10f64.powf(-3.0 * period / t60)
    }
}

/// The largest loop-filter gain: the DC loop always decays.
const MAX_GAIN: f64 = 0.999_99;

/// A tuned loop: the line's length, and the one-pole loss filter
/// `g(1 − a)/(1 − a·z⁻¹)` in it.
#[derive(Copy, Clone, Debug)]
struct Tuning {
    delay: f32,
    pole: f64,
    gain: f64,
}

/// Tune a loop of `period` samples whose line reads through `read`:
/// at `w0` (radians/sample) the whole loop — line, interpolator and loss
/// filter — has a phase delay of `period` and the gain `g0`, and, given
/// `treble = (w1, g1)`, the gain `g1` at `w1` too. The interpolator's
/// response depends on the fraction the delay lands on, so the delay,
/// the pole and the gain are solved together, by fixed-point iteration
/// (the fraction moves by less than the interpolator's phase error, so
/// three passes settle it to f32).
fn tune<I: Interpolator>(read: &I, period: f64, w0: f64, g0: f64, treble: Option<(f64, f64)>) -> Tuning {
    let polar = |(re, im): (f64, f64), w: f64| ((re * re + im * im).sqrt(), -im.atan2(re) / w);
    let mut d = period;
    let mut t = Tuning { delay: period as f32, pole: 0.0, gain: g0.min(MAX_GAIN) };
    for _ in 0..4 {
        let whole = d.floor();
        let frac = (d - whole) as f32;
        let (m0, tau_read) = polar(read.response(frac, w0), w0);
        let target0 = g0 / m0;
        let mut a = 0.0;
        if let Some((w1, g1)) = treble {
            let (m1, _) = polar(read.response(frac, w1), w1);
            let rho = (g1 / m1) / target0;
            if rho < 1.0 && target0 > 0.0 {
                let r2 = rho * rho;
                let beta = (w0.cos() - r2 * w1.cos()) / (r2 - 1.0);
                if beta < -1.0 {
                    a = -beta - (beta * beta - 1.0).sqrt();
                }
            }
        }
        let (s, c) = w0.sin_cos();
        let gain = (target0 * (1.0 - 2.0 * a * c + a * a).sqrt() / (1.0 - a)).min(MAX_GAIN);
        let tau_loss = (a * s).atan2(1.0 - a * c) / w0;
        d = (d + period - (whole + tau_read + tau_loss)).max(I::NEWER as f64);
        t = Tuning { delay: d as f32, pole: a, gain };
    }
    t
}

/// Line length for a loop of `lowest` Hz at `sample_rate`, with room for
/// an interpolator's older taps.
fn capacity(lowest: f32, sample_rate: f32, older: usize) -> usize {
    (f64::from(sample_rate) / f64::from(lowest.max(1.0))).ceil() as usize + older + 4
}

#[cfg(test)]
mod tests;
