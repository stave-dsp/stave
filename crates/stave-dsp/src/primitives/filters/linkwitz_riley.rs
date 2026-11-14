//! Linkwitz-Riley crossover filters.
//!
//! [`LinkwitzRiley4`] is two cascaded Butterworth biquads (Q = 1/√2
//! each): 24 dB/octave, −6 dB at the corner. The defining property:
//! a matched LR4 lowpass and highpass at the same corner sum to a flat
//! magnitude (a second-order allpass), so a split and recombine is
//! unity gain everywhere, rotating phase only around the corner.
//! [`Crossover3`] builds the three-band split on it.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::units::Hz;

const BUTTERWORTH_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// Which side of the corner a [`LinkwitzRiley4`] passes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CrossoverSide {
    Lowpass,
    Highpass,
}

/// One side of a fourth-order Linkwitz–Riley crossover.
///
/// # Models
///
/// Linkwitz and Riley's fourth-order crossover filter (1976): a squared
/// second-order Butterworth, 24 dB/octave, −6 dB at the corner.
///
/// # Derivation
///
/// Two cascaded cookbook Butterworth sections (`Q = 1/√2`), bilinear,
/// prewarped at the corner.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/linkwitz_riley.rs::lr4_pair_sums_flat_and_is_minus_6_db_at_the_corner`:
/// a matched pair sums within 0.01 dB of flat from 60 Hz to 15 kHz, and
/// the corner reads −6.02 dB within 0.05.
///
/// # Cost
///
/// No datasheet line yet (DSP.md P1): two biquads per side, about twice
/// `"Biquad (lowpass)"`.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// Corner held inside (1 Hz, 0.499·fs) by the sections; any rate.
#[derive(Clone, Copy, Debug)]
pub struct LinkwitzRiley4 {
    side: CrossoverSide,
    a: Biquad,
    b: Biquad,
}

impl LinkwitzRiley4 {
    pub fn new(side: CrossoverSide, corner: Hz) -> Self {
        let spec = Self::spec(side, corner);
        Self {
            side,
            a: Biquad::new(spec),
            b: Biquad::new(spec),
        }
    }

    pub fn lowpass(corner: Hz) -> Self {
        Self::new(CrossoverSide::Lowpass, corner)
    }

    pub fn highpass(corner: Hz) -> Self {
        Self::new(CrossoverSide::Highpass, corner)
    }

    /// Move the corner (state kept).
    pub fn set_frequency(&mut self, corner: Hz) {
        let spec = Self::spec(self.side, corner);
        self.a.set(spec);
        self.b.set(spec);
    }

    fn spec(side: CrossoverSide, frequency: Hz) -> FilterSpec {
        let q = BUTTERWORTH_Q;
        match side {
            CrossoverSide::Lowpass => FilterSpec::Lowpass { frequency, q },
            CrossoverSide::Highpass => FilterSpec::Highpass { frequency, q },
        }
    }
}

impl Stage for LinkwitzRiley4 {
    fn prepare(&mut self, sample_rate: f32) {
        self.a.prepare(sample_rate);
        self.b.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.b.tick(self.a.tick(x))
    }
    fn tail(&self) -> Tail {
        self.a.tail().then(self.b.tail())
    }
}

/// A three-band Linkwitz-Riley split at `low` and `high`:
/// low = LP(low) through an allpass at `high` (so its phase matches the
/// other two bands), mid = HP(low)·LP(high), high = HP(low)·HP(high).
/// The bands sum to a flat magnitude; [`split`](Self::split) returns them,
/// and the [`Stage`] view ticks their sum.
#[derive(Clone, Copy, Debug)]
pub struct Crossover3 {
    low_lp: LinkwitzRiley4,
    low_hp: LinkwitzRiley4,
    high_lp: LinkwitzRiley4,
    high_hp: LinkwitzRiley4,
    /// The low band's phase match: the allpass an LR4 pair sums to.
    low_ap: Biquad,
}

impl Crossover3 {
    pub fn new(low: Hz, high: Hz) -> Self {
        debug_assert!(low < high, "crossover {low} ≥ {high}");
        Self {
            low_lp: LinkwitzRiley4::lowpass(low),
            low_hp: LinkwitzRiley4::highpass(low),
            high_lp: LinkwitzRiley4::lowpass(high),
            high_hp: LinkwitzRiley4::highpass(high),
            low_ap: Biquad::new(FilterSpec::Allpass {
                frequency: high,
                q: BUTTERWORTH_Q,
            }),
        }
    }

    /// Move both corners (state kept).
    pub fn set_corners(&mut self, low: Hz, high: Hz) {
        debug_assert!(low < high, "crossover {low} ≥ {high}");
        self.low_lp.set_frequency(low);
        self.low_hp.set_frequency(low);
        self.high_lp.set_frequency(high);
        self.high_hp.set_frequency(high);
        self.low_ap.set(FilterSpec::Allpass {
            frequency: high,
            q: BUTTERWORTH_Q,
        });
    }

    /// One input sample split into `[low, mid, high]`.
    #[inline]
    pub fn split(&mut self, x: f32) -> [f32; 3] {
        let low = self.low_ap.tick(self.low_lp.tick(x));
        let rest = self.low_hp.tick(x);
        [low, self.high_lp.tick(rest), self.high_hp.tick(rest)]
    }
}

impl Stage for Crossover3 {
    fn prepare(&mut self, sample_rate: f32) {
        self.low_lp.prepare(sample_rate);
        self.low_hp.prepare(sample_rate);
        self.high_lp.prepare(sample_rate);
        self.high_hp.prepare(sample_rate);
        self.low_ap.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.low_lp.reset();
        self.low_hp.reset();
        self.high_lp.reset();
        self.high_hp.reset();
        self.low_ap.reset();
    }
    /// The recombined bands: an allpass of the input.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let [l, m, h] = self.split(x);
        l + m + h
    }
    fn tail(&self) -> Tail {
        let low = self.low_lp.tail().then(self.low_ap.tail());
        let mid = self.low_hp.tail().then(self.high_lp.tail());
        let high = self.low_hp.tail().then(self.high_hp.tail());
        low.max(mid).max(high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::hz;

    fn tone_db(f: &mut impl FnMut(f32) -> f32, hz: f32, fs: f32) -> f64 {
        let n = (fs as usize) / 2;
        let (mut acc_in, mut acc_out) = (0.0f64, 0.0f64);
        for i in 0..2 * n {
            let x =
                (core::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(fs)).sin() as f32;
            let y = f(x);
            if i >= n {
                acc_in += f64::from(x) * f64::from(x);
                acc_out += f64::from(y) * f64::from(y);
            }
        }
        10.0 * (acc_out / acc_in).log10()
    }

    #[test]
    fn lr4_pair_sums_flat_and_is_minus_6_db_at_the_corner() {
        let fs = 48_000.0;
        for hz in [60.0f32, 400.0, 1_000.0, 5_000.0, 15_000.0] {
            let (mut lp, mut hp) = (
                LinkwitzRiley4::lowpass(Hz::new(1_000.0)),
                LinkwitzRiley4::highpass(Hz::new(1_000.0)),
            );
            lp.prepare(fs);
            hp.prepare(fs);
            let g = tone_db(&mut |x| lp.tick(x) + hp.tick(x), hz, fs);
            assert!(g.abs() < 0.01, "sum at {hz} Hz: {g:.4} dB");
        }
        let mut lp = LinkwitzRiley4::lowpass(hz(1_000.0));
        lp.prepare(fs);
        let g = tone_db(&mut |x| lp.tick(x), 1_000.0, fs);
        assert!((g + 6.02).abs() < 0.05, "corner {g:.3} dB");
    }

    #[test]
    fn crossover3_bands_sum_flat_and_separate() {
        let fs = 48_000.0;
        for hz in [40.0f32, 200.0, 900.0, 2_500.0, 8_000.0, 18_000.0] {
            let mut x3 = Crossover3::new(Hz::new(250.0), Hz::new(2_500.0));
            x3.prepare(fs);
            let g = tone_db(&mut |x| x3.tick(x), hz, fs);
            assert!(g.abs() < 0.01, "recombined at {hz} Hz: {g:.4} dB");
        }
        // Each band owns its region: a tone two octaves inside a band
        // leaves the other bands ≥ 30 dB down.
        for (hz, band) in [(60.0f32, 0usize), (800.0, 1), (10_000.0, 2)] {
            for other in 0..3 {
                let mut x3 = Crossover3::new(Hz::new(250.0), Hz::new(2_500.0));
                x3.prepare(fs);
                let g = tone_db(&mut |x| x3.split(x)[other], hz, fs);
                if other == band {
                    assert!(g > -1.0, "band {band} at {hz} Hz: {g:.2} dB");
                } else {
                    assert!(g < -30.0, "band {other} leaks {g:.1} dB at {hz} Hz");
                }
            }
        }
    }
}
