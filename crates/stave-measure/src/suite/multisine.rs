//! Odd-odd multisines (Pintelon & Schoukens, *System Identification*,
//! 2nd ed., §3.4 and ch. 4): equal-amplitude tones on bins ≡ 1 (mod 4),
//! log-spaced (at least three to an octave), with a detection line on
//! each side of parity between consecutive excited lines.
//!
//! The phase law is part of the stimulus ([`Phases`]): the best linear
//! approximation depends on the amplitude distribution, so random-phase
//! and Schroeder multisines are not interchangeable (Schoukens, Vaes &
//! Pintelon 2016). C3 uses seeded random phases, averaged over
//! realisations at T1; low-crest Schroeder phases are the cross-check
//! against a sine's compression (C4).
//!
//! One run gives, per excited frequency, the best linear approximation
//! (magnitude and phase), and on the unexcited bins the distortion
//! sorted by parity: a product of an even number of excited tones lands
//! on an even bin, of an odd number on an odd one — those on bins ≡ 3
//! (mod 4) are never excited, so they read the odd-order distortion alone.

use std::f64::consts::TAU;

use super::excitation::{Excitation, Tone};
use crate::spectrum::fft;
use super::period::Period;

/// A multisine's phase law.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phases {
    /// Uniform random phases, one realisation per seed (C3).
    Random { seed: u64 },
    /// Low-crest Schroeder phases ([`Multisine::phases`]): the
    /// cross-check against C4.
    Schroeder,
}

/// An odd-odd multisine's bins on a period.
#[derive(Clone, Debug, PartialEq)]
pub struct Multisine {
    pub period: Period,
    /// The excited bins, ascending, each ≡ 1 (mod 4).
    pub bins: Vec<u32>,
}

impl Multisine {
    /// Tones from `low_hz` to `high_hz`, `per_octave` to an octave
    /// (log-spaced; at the bottom, where the grid is coarser than the
    /// spacing, every bin ≡ 1 (mod 4) instead).
    pub fn odd_odd(period: Period, low_hz: f64, high_hz: f64, per_octave: f64) -> Self {
        let mut bins: Vec<u32> = Vec::new();
        let octaves = (high_hz / low_hz).log2();
        let count = (octaves * per_octave).floor() as u32;
        for j in 0..=count {
            let hz = low_hz * 2f64.powf(f64::from(j) / per_octave);
            let mut k = period.bin_near(hz);
            // The nearest bin ≡ 1 (mod 4) at or above the last one.
            k = k - (k + 3) % 4;
            if k == 0 {
                k = 1;
            }
            while bins.last().is_some_and(|&last| k <= last) {
                k += 4;
            }
            if period.hz(k) <= high_hz * 1.0001 {
                bins.push(k);
            }
        }
        Self { period, bins }
    }

    /// The even detection bins: one above each excited bin (≡ 2 mod 4).
    pub fn even_detection(&self) -> Vec<u32> {
        self.bins.iter().map(|k| k + 1).collect()
    }

    /// The odd detection bins: two above each excited bin (≡ 3 mod 4).
    pub fn odd_detection(&self) -> Vec<u32> {
        self.bins.iter().map(|k| k + 2).collect()
    }

    /// Every bin a run is read at: DC, the excited and detection bins.
    pub fn read_bins(&self) -> Vec<u32> {
        let mut v = vec![0];
        for &k in &self.bins {
            v.extend([k, k + 1, k + 2]);
        }
        v
    }

    /// The tones at total RMS `rms` (equal amplitudes) with `phases`.
    pub fn excitation(&self, rms: f64, phases: Phases) -> Excitation {
        let amplitude = rms * (2.0 / self.bins.len() as f64).sqrt();
        let law = match phases {
            Phases::Random { seed } => self.random_phases(seed),
            Phases::Schroeder => self.phases(),
        };
        let tones = self
            .bins
            .iter()
            .zip(law)
            .map(|(&b, phase)| Tone::new(amplitude, b).phased(phase))
            .collect();
        Excitation::new(self.period, tones)
    }

    /// Phases drawn uniformly on [0, 2π) from `seed` (SplitMix64): one
    /// realisation of a random-phase multisine, the same on every
    /// platform.
    pub fn random_phases(&self, seed: u64) -> Vec<f64> {
        let mut state = seed;
        self.bins
            .iter()
            .map(|_| {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                TAU * (z >> 11) as f64 / (1u64 << 53) as f64
            })
            .collect()
    }

    /// Low-crest phases: Schroeder's for unevenly spaced bins,
    /// `φ_k = −(2π/F)·Σ_{l<k} (b_k − b_l)` (a chirp's, each tone an equal
    /// share of the period), then 100 rounds of clipping the waveform at
    /// 80 % of its peak and keeping the clipped signal's phases (Van der
    /// Ouderaa, Schoukens & Renneboog, 1988). Deterministic.
    pub fn phases(&self) -> Vec<f64> {
        let f = self.bins.len();
        let mut phase: Vec<f64> = self
            .bins
            .iter()
            .enumerate()
            .map(|(k, &b)| {
                let spread: f64 = self.bins[..k].iter().map(|&l| f64::from(b - l)).sum();
                -TAU * spread / f as f64
            })
            .collect();
        let top = self.bins.last().copied().unwrap_or(1) as usize;
        let n = (8 * top).next_power_of_two().max(4096);
        let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
        for _ in 0..100 {
            // x = Σ sin(2π b i/n + φ): the inverse FFT of n/2·e^{j(φ−π/2)}
            // at b and its conjugate at n − b, by a forward FFT of the
            // conjugate (the result is real).
            re.iter_mut().for_each(|v| *v = 0.0);
            im.iter_mut().for_each(|v| *v = 0.0);
            for (&b, &p) in self.bins.iter().zip(&phase) {
                let (s, c) = (p - std::f64::consts::FRAC_PI_2).sin_cos();
                let b = b as usize;
                (re[b], im[b]) = (0.5 * c, -0.5 * s);
                (re[n - b], im[n - b]) = (0.5 * c, 0.5 * s);
            }
            fft(&mut re, &mut im);
            let peak = re.iter().fold(0.0, |m: f64, v| m.max(v.abs())) * 0.8;
            re.iter_mut().for_each(|v| *v = v.clamp(-peak, peak));
            im.iter_mut().for_each(|v| *v = 0.0);
            fft(&mut re, &mut im);
            for (&b, p) in self.bins.iter().zip(phase.iter_mut()) {
                // sin(θ + φ) has the DFT coefficient's argument φ − π/2.
                *p = im[b as usize].atan2(re[b as usize]) + std::f64::consts::FRAC_PI_2;
            }
        }
        phase
    }

    /// Peak over RMS of the unit-RMS excitation with `phases`, sampled at
    /// 32 points a cycle of the highest tone (to about 0.5 %).
    pub fn crest_factor(&self, phases: Phases) -> f64 {
        let e = self.excitation(1.0, phases);
        let n = 32 * self.bins.last().copied().unwrap_or(1) as usize;
        (0..n)
            .map(|i| e.at(i as f64 * self.period.seconds() / n as f64).abs())
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bins_are_odd_odd_log_spaced_and_detectable() {
        let p = Period::units(15); // 50 ms: 20 Hz bins
        let m = Multisine::odd_odd(p, 20.0, 20_000.0, 3.0);
        assert!(m.bins.iter().all(|k| k % 4 == 1));
        assert!(m.bins.windows(2).all(|w| w[0] < w[1]));
        assert!(m.bins.len() >= 20, "{}", m.bins.len());
        assert!(p.hz(*m.bins.last().unwrap()) <= 20_000.0 * 1.0001);
        // No detection bin is excited.
        for d in m.even_detection().iter().chain(&m.odd_detection()) {
            assert!(!m.bins.contains(d));
        }
    }

    #[test]
    fn schroeder_phases_keep_the_crest_low() {
        let m = Multisine::odd_odd(Period::units(15), 20.0, 20_000.0, 3.0);
        let crest = m.crest_factor(Phases::Schroeder);
        // Random or plain Schroeder phases sit near 3 for a sparse log grid;
        // equal phases at √(2F) ≈ 7.7.
        assert!(crest < 2.6, "crest factor {crest}");
        let e = m.excitation(0.3, Phases::Random { seed: 1 });
        assert!((e.rms() - 0.3).abs() < 1e-12);
        // Random phases: a Gaussian-like amplitude distribution, a higher
        // crest, and a different draw per seed.
        let random = m.crest_factor(Phases::Random { seed: 1 });
        assert!(random > crest && random < 5.0, "{random}");
        assert_ne!(m.random_phases(1), m.random_phases(2));
        assert!(m.random_phases(7).iter().all(|p| (0.0..TAU).contains(p)));
    }
}
