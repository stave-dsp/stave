//! Fractional-delay interpolators: how a [`DelayLine`] reads between
//! its samples.
//!
//! An interpolator is the delay line's type parameter
//! (`DelayLine<Lagrange3>`), so the read a modulated delay pays for is
//! named in its type:
//!
//! | Interpolator | Kind | Taps | Character |
//! |---|---|---|---|
//! | [`Linear`] | FIR, order 1 | 2 | cheapest; a lowpass that moves with the fraction |
//! | [`Lagrange3`] | FIR, order 3 | 4 | maximally flat at DC; the chorus and flanger default |
//! | [`Lagrange5`] | FIR, order 5 | 6 | flatter into the treble |
//! | [`Thiran`] | IIR allpass, order 1 | 3 | flat magnitude, phase error only; one read per line |
//! | [`Sinc`] | windowed-sinc FIR | `N` | the closest to ideal, at `N` multiplies |
//!
//! Every interpolator sees the same window of samples: `taps[j]` is the
//! sample `i − NEWER + j` samples old, where `i` is the whole part of the
//! delay, and returns the signal `i + frac` samples old.
//!
//! [`DelayLine`]: super::delay::DelayLine

use core::fmt::Debug;

use crate::kernel::denormal;

/// The most taps an interpolator may read (the window [`DelayLine`]
/// gathers on the stack).
///
/// [`DelayLine`]: super::delay::DelayLine
pub const MAX_TAPS: usize = 32;

/// A fractional-delay read: the weights (or recursion) that estimate a
/// signal between its samples.
///
/// # Models
/// Band-limited reconstruction of a sampled signal at a fractional
/// delay, `x(n − i − frac)`, from the `NEWER + OLDER + 1` samples
/// around it.
///
/// # Derivation
/// Each implementation names its textbook design (Laakso, Välimäki,
/// Karjalainen & Laine, "Splitting the unit delay", IEEE SPM 1996).
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::weights_sum_to_one`,
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets`
/// and `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::modulated_reads_stay_finite`: the weights sum to one, a
/// sine's fractional-delay magnitude and phase errors stay inside each
/// interpolator's budget, and a modulated read stays finite at extreme
/// rates.
///
/// # Cost
/// Per implementation (`tests/datasheet.rs`, `Delay<…>` lines).
///
/// # Aliasing
/// None: linear and time-invariant at a fixed delay. A modulated delay
/// is a time-varying resampler; its images are the interpolator's
/// stopband leakage.
///
/// # Valid ranges
/// `0 ≤ frac < 1`; the delay line keeps every read at least `NEWER` and
/// at most `capacity − 1 − OLDER` samples old.
pub trait Interpolator: Clone + Default + Debug {
    /// Samples newer than the whole part of the delay the read uses: the
    /// shortest delay it can read.
    const NEWER: usize;
    /// Samples older than the whole part of the delay the read uses.
    const OLDER: usize;
    /// Samples the read keeps ringing after its window has passed (an
    /// IIR interpolator's own decay; zero for FIR).
    const SETTLE: u32 = 0;

    /// Build any tables. Called by [`DelayLine::allocate`], never on the
    /// audio thread.
    ///
    /// [`DelayLine::allocate`]: super::delay::DelayLine::allocate
    fn prepare(&mut self) {}

    /// Clear any state (an IIR interpolator's memory).
    fn reset(&mut self) {}

    /// The signal `NEWER + frac` samples older than `taps[0]`, from
    /// `taps.len() == NEWER + OLDER + 1` consecutive samples, newest
    /// first.
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32;
}

/// Straight-line interpolation between the two nearest samples.
///
/// # Models
/// First-order Lagrange interpolation: `x₀ + (x₁ − x₀)·frac`.
///
/// # Derivation
/// Textbook (Laakso et al. 1996, §III-B, N = 1).
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets` (worst
/// over fractions 0.05–0.95): 0.44 dB and 0.0007 cycles at
/// 0.1·`sample_rate`; 3.0 dB and 0.012 cycles at 0.25·`sample_rate`
/// (budgets 0.45 dB / 0.001 and 3.05 dB / 0.012).
///
/// # Cost
/// `Delay<Linear>`: the datasheet's cheapest delay read.
///
/// # Aliasing
/// None at a fixed delay; a modulated read leaks images through its
/// sinc² stopband.
///
/// # Valid ranges
/// Any delay from 0 samples.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Linear;

impl Interpolator for Linear {
    const NEWER: usize = 0;
    const OLDER: usize = 1;

    #[inline]
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32 {
        let (a, b) = (taps[0], taps[1]);
        a + (b - a) * frac
    }
}

/// The Lagrange weights `hₖ = Π_{m≠k} (d − m)/(k − m)` for `P` points at
/// positions `0..P`, evaluated at `d`. They sum to one for any `d` (the
/// interpolant of a constant is that constant).
#[inline]
fn lagrange_weights<const P: usize>(d: f32) -> [f32; P] {
    let mut h = [1.0f32; P];
    for (k, hk) in h.iter_mut().enumerate() {
        for m in 0..P {
            if m != k {
                *hk *= (d - m as f32) / (k as f32 - m as f32);
            }
        }
    }
    h
}

/// Third-order (four-point) Lagrange interpolation, centred so the read
/// falls in the middle interval.
///
/// # Models
/// The cubic through the four samples around the read point: maximally
/// flat magnitude and group delay at DC.
///
/// # Derivation
/// Laakso et al. 1996, eq. (42), N = 3, with `D = 1 + frac` (the middle
/// interval, where odd-order Lagrange is passive: `|H| ≤ 1`).
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::weights_sum_to_one` and
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets`: weights sum to one within 1e-6; worst over
/// fractions, 0.031 dB and 0.00005 cycles at 0.1·`sample_rate`, 1.07 dB
/// and 0.0045 cycles at 0.25·`sample_rate` (budgets 0.035 dB / 0.0001
/// and 1.1 dB / 0.005).
///
/// # Cost
/// `Delay<Lagrange3>` on the datasheet.
///
/// # Aliasing
/// None at a fixed delay; a modulated read's images are attenuated by
/// the cubic's stopband (more than [`Linear`]'s, less than [`Sinc`]'s).
///
/// # Valid ranges
/// Delays from 1 sample.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lagrange3;

impl Lagrange3 {
    /// The four weights for `frac`, newest tap first.
    pub fn weights(frac: f32) -> [f32; 4] {
        lagrange_weights::<4>(1.0 + frac)
    }
}

impl Interpolator for Lagrange3 {
    const NEWER: usize = 1;
    const OLDER: usize = 2;

    #[inline]
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32 {
        let h = Self::weights(frac);
        h[0] * taps[0] + h[1] * taps[1] + h[2] * taps[2] + h[3] * taps[3]
    }
}

/// Fifth-order (six-point) Lagrange interpolation, centred.
///
/// # Models
/// The quintic through the six samples around the read point.
///
/// # Derivation
/// Laakso et al. 1996, eq. (42), N = 5, `D = 2 + frac`.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::weights_sum_to_one` and
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets`: weights sum to one within 1e-6; worst over
/// fractions, 0.0025 dB and 0.000004 cycles at 0.1·`sample_rate`,
/// 0.44 dB and 0.0019 cycles at 0.25·`sample_rate` (budgets 0.003 dB /
/// 0.00001 and 0.45 dB / 0.002).
///
/// # Cost
/// `Delay<Lagrange5>` on the datasheet.
///
/// # Aliasing
/// None at a fixed delay; a steeper stopband than [`Lagrange3`] for
/// modulated reads.
///
/// # Valid ranges
/// Delays from 2 samples.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lagrange5;

impl Lagrange5 {
    /// The six weights for `frac`, newest tap first.
    pub fn weights(frac: f32) -> [f32; 6] {
        lagrange_weights::<6>(2.0 + frac)
    }
}

impl Interpolator for Lagrange5 {
    const NEWER: usize = 2;
    const OLDER: usize = 3;

    #[inline]
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32 {
        let h = Self::weights(frac);
        let mut y = 0.0;
        for k in 0..6 {
            y += h[k] * taps[k];
        }
        y
    }
}

/// First-order Thiran allpass interpolation: a flat magnitude at every
/// fraction, the error all in phase.
///
/// # Models
/// `H(z) = (a + z⁻¹)/(1 + a·z⁻¹)`, `a = (1 − d)/(1 + d)`, whose phase
/// delay is maximally flat at DC around `d` samples.
///
/// # Derivation
/// Thiran 1971; as a delay-line read, Jaffe & Smith 1983 and Laakso et
/// al. 1996, §IV. `d` is kept in `[0.5, 1.5)` (the read borrows one
/// sample of the whole delay when `frac < 0.5`) so the pole stays inside
/// `|a| ≤ 1/3`. The previous input is read back from the line, so a
/// moving delay feeds the recursion a consistent signal.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::weights_sum_to_one`,
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets` and
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::modulated_reads_stay_finite`: unity gain at DC; no magnitude error
/// (allpass, budget 0.001 dB); worst phase error 0.0022 cycles at
/// 0.1·`sample_rate` and 0.027 cycles at 0.25·`sample_rate` (budgets
/// 0.0025 and 0.03); a delay swept at audio rates stays finite and
/// bounded.
///
/// # Cost
/// `Delay<Thiran>` on the datasheet.
///
/// # Aliasing
/// None at a fixed delay. Under fast modulation the recursion's state
/// lags the coefficient change (a transient, not an image); prefer
/// Lagrange for deep audio-rate modulation.
///
/// # Valid ranges
/// Delays from 1 sample; one read per line per sample (the recursion is
/// the reader's state).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Thiran {
    y1: f32,
}

impl Thiran {
    /// The allpass coefficient for a fractional delay `d ∈ [0.5, 1.5)`.
    pub fn coefficient(d: f32) -> f32 {
        (1.0 - d) / (1.0 + d)
    }
}

impl Interpolator for Thiran {
    const NEWER: usize = 1;
    const OLDER: usize = 1;
    // |a| ≤ 1/3: (1/3)^13 < 1e-6.
    const SETTLE: u32 = 13;

    fn reset(&mut self) {
        self.y1 = 0.0;
    }

    #[inline]
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32 {
        // taps: [i − 1, i, i + 1] samples old.
        let (now, prev, d) = if frac < 0.5 {
            (taps[0], taps[1], 1.0 + frac)
        } else {
            (taps[1], taps[2], frac)
        };
        let a = Self::coefficient(d);
        let y = a * now + prev - a * self.y1;
        self.y1 = denormal::flush(y);
        self.y1
    }
}

/// Polyphase rows in a [`Sinc`] table (plus one, so the last fraction
/// interpolates toward frac = 1).
const SINC_PHASES: usize = 256;

/// The Kaiser window's shape parameter for [`Sinc`]: ≈ 60 dB sidelobes.
const SINC_KAISER_BETA: f64 = 6.0;

/// Windowed-sinc interpolation over `N` taps (even, 4 ≤ N ≤ 32).
///
/// # Models
/// The ideal band-limited fractional delay, `sinc(t)`, truncated to `N`
/// taps by a Kaiser window (β = 6), each fraction's weights normalised
/// to unity DC gain.
///
/// # Derivation
/// Laakso et al. 1996, §III-A (windowed ideal). The weights are tabled
/// at 256 fractions in [`prepare`](Interpolator::prepare) (one
/// allocation, `(256 + 1)·N` floats) and read with linear interpolation
/// between rows, which stays below −90 dB of the ideal row.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::weights_sum_to_one`,
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sinc_table_matches_the_direct_weights` and
/// `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::sine_errors_stay_inside_their_budgets`: weights sum to one within 1e-5; the table
/// holds the direct weights within 3e-5. Worst over fractions at 0.1
/// and 0.25·`sample_rate`: `Sinc<8>` 0.0055 dB / 0.00002 cycles and
/// 0.0006 dB / 0.00005 cycles; `Sinc<16>` 0.0010 dB / 0.00004 and
/// 0.0030 dB / 0.00002; `Sinc<32>` 0.0040 dB / 0.000003 and
/// 0.0004 dB / 0.00001 (the window's passband ripple dominates).
///
/// # Cost
/// `Delay<Sinc<8>>` and `Delay<Sinc<16>>` on the datasheet: `N`
/// multiplies plus two row reads.
///
/// # Aliasing
/// None at a fixed delay; the stopband of a modulated read is the
/// window's (≈ −60 dB beyond the transition band).
///
/// # Valid ranges
/// Delays from `N/2 − 1` samples; `N` even in `4..=32` (a compile-time
/// error otherwise).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sinc<const N: usize> {
    table: Vec<f32>,
}

impl<const N: usize> Sinc<N> {
    const VALID: () = assert!(
        N % 2 == 0 && N >= 4 && N <= MAX_TAPS,
        "Sinc<N> takes an even N in 4..=32"
    );

    /// The `N` weights for `frac`, newest tap first, computed directly
    /// (the table holds these at 256 fractions).
    pub fn weights(frac: f32) -> [f32; N] {
        let () = Self::VALID;
        let half = (N / 2) as f64;
        let centre = half - 1.0 + f64::from(frac);
        let mut h = [0.0f32; N];
        let mut wide = [0.0f64; N];
        let mut sum = 0.0;
        for (j, w) in wide.iter_mut().enumerate() {
            let t = j as f64 - centre;
            let sinc = if t.abs() < 1e-12 {
                1.0
            } else {
                (core::f64::consts::PI * t).sin() / (core::f64::consts::PI * t)
            };
            let r = (t / half).clamp(-1.0, 1.0);
            let window =
                bessel_i0(SINC_KAISER_BETA * (1.0 - r * r).sqrt()) / bessel_i0(SINC_KAISER_BETA);
            *w = sinc * window;
            sum += *w;
        }
        for (hj, w) in h.iter_mut().zip(wide) {
            *hj = (w / sum) as f32;
        }
        h
    }
}

/// The zeroth-order modified Bessel function of the first kind, by its
/// power series (converges for every real argument; the Kaiser window's
/// arguments stay below 10).
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

impl<const N: usize> Interpolator for Sinc<N> {
    const NEWER: usize = N / 2 - 1;
    const OLDER: usize = N / 2;

    fn prepare(&mut self) {
        let () = Self::VALID;
        if self.table.len() == (SINC_PHASES + 1) * N {
            return;
        }
        self.table = Vec::with_capacity((SINC_PHASES + 1) * N);
        for p in 0..=SINC_PHASES {
            self.table
                .extend_from_slice(&Self::weights(p as f32 / SINC_PHASES as f32));
        }
    }

    #[inline]
    fn interpolate(&mut self, taps: &[f32], frac: f32) -> f32 {
        if self.table.is_empty() {
            // Unprepared: the nearest pair, linearly.
            let i = N / 2 - 1;
            return taps[i] + (taps[i + 1] - taps[i]) * frac;
        }
        let pos = frac * SINC_PHASES as f32;
        let p = (pos as usize).min(SINC_PHASES - 1);
        let t = pos - p as f32;
        let (lo, hi) = self.table[p * N..(p + 2) * N].split_at(N);
        let mut y = 0.0;
        for j in 0..N {
            y += (lo[j] + (hi[j] - lo[j]) * t) * taps[j];
        }
        y
    }
}

#[cfg(test)]
mod tests;
