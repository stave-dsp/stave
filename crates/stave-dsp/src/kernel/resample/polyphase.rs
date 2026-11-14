//! Odd-factor resampling for [`Oversampled`](crate::Oversampled): one
//! Kaiser windowed-sinc lowpass at the inner rate, split into `p`
//! phases to interpolate (each output phase touches only the taps that
//! see signal) and run once per `p` inner samples to decimate (only the
//! kept phase is computed). The design rules, shared with the deeper
//! half-band levels and the IIR half-bands, are [`edges`] and
//! [`kaiser_length`].

use super::{bessel_i0, HB_BETA};

/// The protected band as a fraction of the base (host) rate: 20 kHz at
/// 44.1 kHz, the tightest rate the designs hold (at 48 kHz the same
/// filters pass to 21.8 kHz).
pub(crate) const PASS: f64 = 20_000.0 / 44_100.0;
/// The front stage's stopband edge, a fraction of the base rate: what
/// lies above it folds onto the band above 20 kHz only.
pub(crate) const STOP: f64 = 1.0 - PASS;
/// The stopband every linear-phase design is sized for, dB.
pub(crate) const ATTENUATION_DB: f64 = 96.0;

/// `(pass, stop)` edges, fractions of the BASE rate, for a stage whose
/// outer rate is `r`× the base. The front (`r = 1`) guards the base
/// Nyquist with the standard half-band trade (the shoulder folds onto
/// itself, above 20 kHz). A deeper stage only has to keep its images
/// clear of what the stages outside it pass: everything up to
/// [`STOP`] of the base rate has its first image at `r − STOP`.
pub(crate) fn edges(r: usize) -> (f64, f64) {
    if r == 1 {
        (PASS, STOP)
    } else {
        (PASS, r as f64 - STOP)
    }
}

/// Kaiser's length estimate for [`ATTENUATION_DB`] across a transition
/// `width` cycles per sample wide (Oppenheim & Schafer §7.5.3).
pub(crate) fn kaiser_length(width: f64) -> f64 {
    (ATTENUATION_DB - 7.95) / (14.36 * width) + 1.0
}

/// The ideal lowpass at `cutoff` (cycles per sample) at offset `k`;
/// exactly zero on the sinc's zeros.
fn sinc(cutoff: f64, k: f64) -> f64 {
    if k == 0.0 {
        return 2.0 * cutoff;
    }
    let turns = 2.0 * cutoff * k;
    if turns == turns.round() {
        0.0
    } else {
        (core::f64::consts::TAU * cutoff * k).sin() / (core::f64::consts::PI * k)
    }
}

/// The lowpass for a `p`× stage whose outer rate is `r`× the base:
/// Kaiser windowed sinc (β for [`ATTENUATION_DB`]) with its cutoff
/// mid-transition, unity DC gain, and a length `L` with `(L − 1)` a
/// multiple of `p·r`, so the round trip's `(L − 1)` inner samples are a
/// whole number of base samples.
pub(crate) fn design(p: usize, r: usize) -> Vec<f64> {
    let (pass, stop) = edges(r);
    let rate = (p * r) as f64;
    let width = (stop - pass) / rate;
    let cutoff = 0.5 * (pass + stop) / rate;
    let step = p * r;
    let len = ((kaiser_length(width) - 1.0) / step as f64).ceil() as usize * step + 1;
    let m = ((len - 1) / 2) as f64;
    let inv_i0 = 1.0 / bessel_i0(HB_BETA);
    let mut taps: Vec<f64> = (0..len)
        .map(|i| {
            let k = i as f64 - m;
            let x = k / m;
            sinc(cutoff, k) * bessel_i0(HB_BETA * (1.0 - x * x).max(0.0).sqrt()) * inv_i0
        })
        .collect();
    let dc: f64 = taps.iter().sum();
    for t in &mut taps {
        *t /= dc;
    }
    taps
}

/// The interpolator's ring for a `len`-tap design at `p`×: phase `j`
/// reads its taps `j + p·k` at ages `k < ⌈len/p⌉`.
fn up_ring_len(len: usize, p: usize) -> usize {
    len.div_ceil(p).next_power_of_two()
}

/// The decimator's ring: it reads every tap, ages `i < len`.
fn down_ring_len(len: usize) -> usize {
    len.next_power_of_two()
}

/// The slot of the sample `age` samples older than the newest (at
/// `at`) in a power-of-two ring of `mask + 1` slots.
#[inline(always)]
fn slot(at: usize, age: usize, mask: usize) -> usize {
    (at + mask + 1 - age) & mask
}

/// A `p`× polyphase FIR interpolator and decimator pair around one
/// design. State lives in power-of-two rings sized at construction.
#[derive(Clone)]
pub(crate) struct Polyphase {
    p: usize,
    len: usize,
    /// `phases[j]`: `(k, p·h[j + p·k])` for every nonzero tap of phase `j`.
    phases: Vec<Vec<(usize, f64)>>,
    /// `(i, h[i])` for every nonzero tap: the decimator.
    taps: Vec<(usize, f64)>,
    up_ring: Vec<f32>,
    up_at: usize,
    down_ring: Vec<f32>,
    down_at: usize,
}

impl Polyphase {
    pub fn new(p: usize, r: usize) -> Self {
        let h = design(p, r);
        let len = h.len();
        let phases = (0..p)
            .map(|j| {
                (0..)
                    .map(|k| (k, j + p * k))
                    .take_while(|&(_, i)| i < len)
                    .filter(|&(_, i)| h[i] != 0.0)
                    .map(|(k, i)| (k, p as f64 * h[i]))
                    .collect()
            })
            .collect();
        let taps = h
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, c)| c != 0.0)
            .collect();
        Self {
            p,
            len,
            phases,
            taps,
            up_ring: vec![0.0; up_ring_len(len, p)],
            up_at: 0,
            down_ring: vec![0.0; down_ring_len(len)],
            down_at: 0,
        }
    }

    pub fn factor(&self) -> usize {
        self.p
    }

    /// The round trip's delay at this stage's outer rate: `(L − 1)`
    /// inner samples, exactly.
    pub fn delay(&self) -> f64 {
        (self.len - 1) as f64 / self.p as f64
    }

    /// The round trip's impulse response length at the outer rate.
    pub fn support(&self) -> u32 {
        (2 * (self.len - 1)).div_ceil(self.p) as u32 + 1
    }

    pub fn reset(&mut self) {
        self.up_ring.fill(0.0);
        self.down_ring.fill(0.0);
        self.up_at = 0;
        self.down_at = 0;
    }

    /// `input` (n samples) to `out` (`p·n`), unity passband gain.
    pub fn up(&mut self, input: &[f32], out: &mut [f32]) {
        let mask = self.up_ring.len() - 1;
        for (&x, group) in input.iter().zip(out.chunks_exact_mut(self.p)) {
            self.up_at = (self.up_at + 1) & mask;
            self.up_ring[self.up_at] = x;
            for (y, phase) in group.iter_mut().zip(&self.phases) {
                let mut acc = 0.0f64;
                for &(k, c) in phase {
                    acc += c * f64::from(self.up_ring[slot(self.up_at, k, mask)]);
                }
                *y = acc as f32;
            }
        }
    }

    /// `input` (`p·n` samples) to `out` (n): each output is the filter
    /// at the first inner sample of its group, so the round trip lands
    /// on whole outer samples.
    pub fn down(&mut self, input: &[f32], out: &mut [f32]) {
        let mask = self.down_ring.len() - 1;
        for (y, group) in out.iter_mut().zip(input.chunks_exact(self.p)) {
            self.down_at = (self.down_at + 1) & mask;
            self.down_ring[self.down_at] = group[0];
            let mut acc = 0.0f64;
            for &(i, c) in &self.taps {
                acc += c * f64::from(self.down_ring[slot(self.down_at, i, mask)]);
            }
            *y = acc as f32;
            for &v in &group[1..] {
                self.down_at = (self.down_at + 1) & mask;
                self.down_ring[self.down_at] = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The front 3× design is the 199-tap Nyquist-band filter whose
    /// round trip is 66 outer samples, like the front half-band's; every
    /// third tap from the centre is an exact zero.
    #[test]
    fn the_front_third_band_is_a_nyquist_filter() {
        let h = design(3, 1);
        assert_eq!(h.len(), 199);
        let m = 99;
        for k in (3..=m).step_by(3) {
            assert_eq!(h[m + k], 0.0);
            assert_eq!(h[m - k], 0.0);
        }
        assert_eq!(Polyphase::new(3, 1).delay(), 66.0);
    }

    /// Every design's round trip is a whole number of base samples.
    #[test]
    fn round_trips_are_whole_base_samples() {
        for (p, r) in [(3, 1), (5, 1), (3, 2), (3, 4), (5, 6), (7, 3)] {
            let len = design(p, r).len();
            assert_eq!((len - 1) % (p * r), 0, "p {p} r {r}");
        }
    }
}

/// Kani proof (DSP.md R10; `scripts/kani.sh`): the polyphase index maths.
#[cfg(kani)]
mod proofs {
    use super::*;

    /// For every design to 4096 taps at every factor to 16: each tap
    /// `i` belongs to exactly one phase (`j = i mod p`, at age
    /// `k = ⌊i/p⌋`, `i == j + p·k`, no other phase's taps `j' + p·k'`
    /// reach it) and every age either filter reads is younger than its
    /// ring; and in every power-of-two ring to 2¹³ slots every age lands
    /// in the ring, age 0 at the newest slot, two different ages never in
    /// the same slot.
    #[kani::proof]
    fn every_tap_has_one_phase_and_one_slot() {
        let len: usize = kani::any_where(|l: &usize| (1..=4096).contains(l));
        let p: usize = kani::any_where(|p: &usize| (1..=16).contains(p));
        let i: usize = kani::any_where(|i: &usize| *i < len);
        let (j, k) = (i % p, i / p);
        assert!(j < p && j + p * k == i);
        assert!(k < up_ring_len(len, p) && i < down_ring_len(len));
        let other: usize = kani::any_where(|o: &usize| *o < p);
        if other != j {
            assert!(i < other || (i - other) % p != 0);
        }

        let mask = (1usize << kani::any_where(|b: &u32| *b <= 13)) - 1;
        let at: usize = kani::any_where(|a: &usize| *a <= mask);
        let a: usize = kani::any_where(|a: &usize| *a <= mask);
        let b: usize = kani::any_where(|b: &usize| *b <= mask);
        assert!(slot(at, a, mask) <= mask);
        assert_eq!(slot(at, 0, mask), at);
        if a != b {
            assert_ne!(slot(at, a, mask), slot(at, b, mask));
        }
    }
}
