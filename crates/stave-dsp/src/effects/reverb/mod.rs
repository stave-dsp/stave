//! Algorithmic reverberators: the [`Fdn`], the [`Dattorro`] plate and
//! [`Schroeder`]'s comb-and-allpass designs (with Moorer's lowpass
//! combs). Each takes its decay as a T60 and is wet only: mix the dry
//! signal yourself.

mod dattorro;
mod fdn;
mod schroeder;

pub use dattorro::Dattorro;
pub use fdn::{Fdn, Mixing};
pub use schroeder::Schroeder;

use crate::kernel::node::Tail;

/// The gain that, applied once every `interval` samples, falls 60 dB in
/// `t60_s` seconds at `sample_rate`. An infinite T60 is lossless (1).
fn loop_gain(interval: f32, t60_s: f32, sample_rate: f32) -> f32 {
    if t60_s.is_infinite() {
        return 1.0;
    }
    if t60_s.is_nan() || t60_s <= 0.0 {
        return 0.0;
    }
    10f32.powf(-3.0 * interval / (t60_s * sample_rate))
}

/// A decay of `t60_s` reaches −120 dB in two T60s, after the first
/// pass through the longest path (`first_samples`).
fn decay_tail(t60_s: f32, sample_rate: f32, first_samples: f32) -> Tail {
    if !t60_s.is_finite() {
        return Tail::Infinite;
    }
    let n = 2.0 * f64::from(t60_s.max(0.0)) * f64::from(sample_rate) + f64::from(first_samples);
    if n >= f64::from(u32::MAX) {
        Tail::Infinite
    } else {
        Tail::samples(n.ceil() as u32)
    }
}

#[cfg(test)]
mod tests;
