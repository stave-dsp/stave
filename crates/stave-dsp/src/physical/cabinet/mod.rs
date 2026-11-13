//! Guitar cabinets, microphones and rooms: captured impulse responses
//! blended by microphone and placement, a parametric close microphone on
//! a [`Driver`](super::speaker::Driver) when no capture is at hand, and
//! a room around either.
//!
//! **Captured.** A cabinet miked at a few positions is a set of
//! impulse responses. [`Captured`] convolves the input with each
//! microphone's response (F2–F3's zero-latency convolver), places a
//! microphone *between* its captured positions, and mixes the
//! microphones. Each response is split into a *shape* and an *arrival*,
//! as Kistler & Wightman split a head-related response into a
//! minimum-phase filter and a pure delay ("A model of head-related
//! transfer functions based on principal components analysis and
//! minimum-phase reconstruction", JASA 91(3), 1992): the arrival is
//! interpolated between positions as a delay, the aligned shapes are
//! blended, so a microphone moved halfway between two captures arrives
//! halfway between them instead of sounding both (a comb). A placement
//! change crossfades the convolver's response (Wefers' filter exchange,
//! [`Convolver::swap_response`](crate::Convolver::swap_response)). The
//! microphones are summed as captured, or time-aligned on their arrivals
//! (the "phase" alignment an engineer does by nudging tracks), with each
//! one's polarity, level and level matching. Responses are the user's:
//! read at run time, never part of stave.
//!
//! **Close microphone.** [`CloseMic`] puts a first-order microphone on
//! the axis of a [`Driver`](super::speaker::Driver)'s cone: the exact
//! on-axis pressure and particle velocity of a baffled piston (two
//! delayed copies of the cone's velocity), mixed by the microphone's
//! pattern — which is the proximity effect, exactly, for a piston — an
//! off-axis lowpass from the piston's directivity, and an
//! approximation of the cone's breakup.
//!
//! **Room.** The room around either is an effect built on the reverbs:
//! [`effects::reverb::Room`](crate::effects::reverb::Room) adds a
//! rectangular room's first reflections (image sources, Allen & Berkley
//! 1979) and a late field decaying by Sabine's T60 at Kuttruff's
//! direct-to-reverberant ratio.

mod captured;
mod close_mic;

pub use captured::{Captured, Level, Position, Shape, Timing};
pub use close_mic::{Breakup, CloseMic, Placement, MAX_DISTANCE, MAX_MODES, PISTON_MINUS_3_DB};

use crate::primitives::spectral::{Complex, Fft};
use core::f64::consts::PI;

pub use super::rotary::SPEED_OF_SOUND;

/// A first-order microphone's polar pattern, `A + B·cos θ` with `A + B
/// = 1`: `pressure` is `A`, the share of the output that is pressure;
/// the rest, `B`, is pressure gradient (particle velocity).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Pattern {
    pub pressure: f64,
}

impl Pattern {
    /// Pressure only: no proximity effect.
    pub const OMNI: Self = Self { pressure: 1.0 };
    /// Half and half: the dynamic guitar-cabinet microphone's pattern.
    pub const CARDIOID: Self = Self { pressure: 0.5 };
    /// `0.37 + 0.63·cos θ`.
    pub const SUPERCARDIOID: Self = Self { pressure: 0.37 };
    /// `0.25 + 0.75·cos θ`.
    pub const HYPERCARDIOID: Self = Self { pressure: 0.25 };
    /// Pressure gradient only (a ribbon).
    pub const FIGURE_EIGHT: Self = Self { pressure: 0.0 };

    /// The gradient share `B = 1 − A`.
    pub fn gradient(&self) -> f64 {
        1.0 - self.pressure
    }

    /// The pattern's gain at angle `theta` (radians) off its axis.
    pub fn gain(&self, theta: f64) -> f64 {
        self.pressure + self.gradient() * theta.cos()
    }
}

/// Half the width of the windowed-sinc kernel [`shift`] uses, samples.
pub const SHIFT_HALF_WIDTH: usize = 16;
/// Its Kaiser window's β.
const SHIFT_BETA: f64 = 8.0;

/// The zeroth-order modified Bessel function of the first kind.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// The windowed-sinc weights reading a signal at `frac` (in `[0, 1)`)
/// past sample `m`: `w[j]` multiplies sample `m − H + 1 + j`, `H` =
/// [`SHIFT_HALF_WIDTH`]. Normalised to sum to 1 (so DC passes exactly).
fn sinc_weights(frac: f64) -> [f64; 2 * SHIFT_HALF_WIDTH] {
    let h = SHIFT_HALF_WIDTH as f64;
    let mut w = [0.0; 2 * SHIFT_HALF_WIDTH];
    let mut sum = 0.0;
    let i0 = bessel_i0(SHIFT_BETA);
    for (j, wj) in w.iter_mut().enumerate() {
        let t = (j as f64 - (h - 1.0)) - frac;
        let sinc = if t.abs() < 1e-12 { 1.0 } else { (PI * t).sin() / (PI * t) };
        let r = (t / h).clamp(-1.0, 1.0);
        *wj = sinc * bessel_i0(SHIFT_BETA * (1.0 - r * r).sqrt()) / i0;
        sum += *wj;
    }
    for wj in &mut w {
        *wj /= sum;
    }
    w
}

/// `out[n] = src(n − by)`: `src` delayed by `by` samples (advanced when
/// negative), zero outside it. A whole number of samples is a copy,
/// exact; a fraction reads through a Kaiser-windowed sinc of
/// ±[`SHIFT_HALF_WIDTH`] samples (passband to 0.45 of the rate).
/// Allocation-free.
pub fn shift(src: &[f64], by: f64, out: &mut [f64]) {
    let whole = by.floor();
    let frac = by - whole;
    let whole = whole as i64;
    if frac == 0.0 {
        for (n, o) in out.iter_mut().enumerate() {
            let m = n as i64 - whole;
            *o = if m >= 0 && (m as usize) < src.len() { src[m as usize] } else { 0.0 };
        }
        return;
    }
    // src(n − whole − frac) = src(m + (1 − frac)) with m = n − whole − 1.
    let w = sinc_weights(1.0 - frac);
    let hw = SHIFT_HALF_WIDTH as i64;
    for (n, o) in out.iter_mut().enumerate() {
        let m = n as i64 - whole - 1;
        let mut acc = 0.0;
        for (j, &wj) in w.iter().enumerate() {
            let k = m - hw + 1 + j as i64;
            if k >= 0 && (k as usize) < src.len() {
                acc += wj * src[k as usize];
            }
        }
        *o = acc;
    }
}

/// The band-limited value of `y` at the fractional index `t`.
fn read(y: &[f64], t: f64) -> f64 {
    let m = t.floor();
    let w = sinc_weights(t - m);
    let hw = SHIFT_HALF_WIDTH as i64;
    let m = m as i64;
    let mut acc = 0.0;
    for (j, &wj) in w.iter().enumerate() {
        let k = m - hw + 1 + j as i64;
        if k >= 0 && (k as usize) < y.len() {
            acc += wj * y[k as usize];
        }
    }
    acc
}

/// The cross-correlation `r[k] = Σ a[n]·b[n − k]` for lags `k` in
/// `−(len b − 1) ..= len a − 1`, returned with the lag of index 0.
fn correlate(a: &[f64], b: &[f64]) -> (Vec<f64>, i64) {
    let n = (a.len() + b.len()).next_power_of_two().max(2);
    let fft = Fft::<f64>::new(n);
    let (mut fa, mut fb) = (vec![0.0; n], vec![0.0; n]);
    fa[..a.len()].copy_from_slice(a);
    fb[..b.len()].copy_from_slice(b);
    let (mut sa, mut sb) = (vec![Complex::default(); n / 2 + 1], vec![Complex::default(); n / 2 + 1]);
    fft.forward_real(&fa, &mut sa);
    fft.forward_real(&fb, &mut sb);
    for (x, y) in sa.iter_mut().zip(&sb) {
        *x = *x * y.conj();
    }
    let mut r = vec![0.0; n];
    fft.inverse_real(&mut sa, &mut r);
    // Circular lags: k ≥ 0 at r[k], k < 0 at r[n + k].
    let neg = b.len() - 1;
    let mut out = Vec::with_capacity(neg + a.len());
    out.extend(r[n - neg..].iter().map(|v| v / n as f64));
    out.extend(r[..a.len()].iter().map(|v| v / n as f64));
    (out, -(neg as i64))
}

/// The delay of `a` relative to `b`, samples: the lag of their
/// band-limited cross-correlation's peak, refined between samples by a
/// golden-section search on the windowed-sinc interpolation of the
/// correlation (accurate to a few thousandths of a sample for signals
/// band-limited to 0.45 of the rate).
pub fn delay_of(a: &[f64], b: &[f64]) -> f64 {
    let (r, first) = correlate(a, b);
    let peak = r
        .iter()
        .enumerate()
        .max_by(|x, y| x.1.total_cmp(y.1))
        .map_or(0, |(i, _)| i);
    let f = |t: f64| read(&r, t);
    let (mut lo, mut hi) = (peak as f64 - 1.0, peak as f64 + 1.0);
    let g = (5.0f64.sqrt() - 1.0) / 2.0;
    let (mut c, mut d) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..60 {
        if fc > fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - g * (hi - lo);
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + g * (hi - lo);
            fd = f(d);
        }
    }
    0.5 * (lo + hi) + first as f64
}

/// The floor (re its peak) under a response's magnitude when its
/// minimum-phase reference is taken for [`arrival`]: −80 dB, about a
/// good capture's noise floor.
pub const ARRIVAL_FLOOR: f64 = 1e-4;

/// A response's arrival, samples: its delay against its own
/// minimum-phase version (the excess-phase response's linear part,
/// Kistler & Wightman's pure delay), so two captures of one shape at
/// different distances differ in arrival by exactly their delay. The
/// minimum-phase reference is taken with the magnitude floored at
/// [`ARRIVAL_FLOOR`]: below it, a capture's stopband is noise, and its
/// depth would otherwise move the reference's phase.
pub fn arrival(h: &[f32]) -> f64 {
    let h: Vec<f64> = h.iter().map(|&v| f64::from(v)).collect();
    let m = crate::primitives::spectral::cepstrum::minimum_phase_floored(&h, ARRIVAL_FLOOR);
    delay_of(&h, &m)
}

#[cfg(test)]
mod tests;
