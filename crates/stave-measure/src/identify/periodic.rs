//! Periodic records: random-phase multisines, their steady-state
//! responses, and exact band-limited changes of rate for periodic
//! signals (a period's spectrum, padded or cut).

use std::f64::consts::TAU;

use stave_dsp::Fft;

use crate::suite::Complex;

/// The spectrum of a real period (the full complex transform).
pub fn spectrum(x: &[f64]) -> Vec<Complex<f64>> {
    let mut data: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v, 0.0)).collect();
    Fft::new(x.len()).forward(&mut data);
    data
}

/// The real period whose spectrum is `s` (the real part of the inverse,
/// normalised).
pub fn waveform(s: &[Complex<f64>]) -> Vec<f64> {
    let n = s.len();
    let mut data = s.to_vec();
    Fft::new(n).inverse(&mut data);
    data.iter().map(|z| z.re / n as f64).collect()
}

/// A deterministic generator (SplitMix64): the stimulus is the same on
/// every machine.
#[derive(Clone, Debug)]
pub struct Seeded(u64);

impl Seeded {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Uniform in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u = self.uniform().max(1e-300);
        let v = self.uniform();
        (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
    }
}

/// The odd bins a multisine of `len` samples at `rate` excites from
/// `low_hz` to `high_hz`: even-order distortion lands on even bins, so
/// the excited ones read the best linear approximation free of it.
pub fn odd_bins(len: usize, rate: f64, low_hz: f64, high_hz: f64) -> Vec<usize> {
    let df = rate / len as f64;
    let lo = ((low_hz / df).ceil() as usize).max(1);
    let hi = ((high_hz / df).floor() as usize).min(len / 2 - 1);
    (lo..=hi).filter(|k| k % 2 == 1).collect()
}

/// One period of a random-phase multisine: equal amplitudes on
/// [`odd_bins`], uniform random phases from `seed`, scaled to `rms`
/// (Schoukens, Vaes & Pintelon 2016: Riemann-equivalent to Gaussian
/// noise of that spectrum).
pub fn multisine(len: usize, rate: f64, low_hz: f64, high_hz: f64, rms: f64, seed: u64) -> Vec<f64> {
    let bins = odd_bins(len, rate, low_hz, high_hz);
    let mut rng = Seeded::new(seed);
    let mut s = vec![Complex::new(0.0, 0.0); len];
    for &k in &bins {
        let p = TAU * rng.uniform();
        s[k] = Complex::new(p.cos(), p.sin());
        s[len - k] = s[k].conj();
    }
    let x = waveform(&s);
    let r = (x.iter().map(|v| v * v).sum::<f64>() / len as f64).sqrt();
    x.iter().map(|v| v * rms / r).collect()
}

/// A period at `to` samples: its spectrum below both Nyquists kept,
/// the rest dropped (exact for a periodic signal, band-limited going
/// down).
pub fn resample(x: &[f64], to: usize) -> Vec<f64> {
    let from = x.len();
    if from == to {
        return x.to_vec();
    }
    let s = spectrum(x);
    let keep = from.min(to) / 2;
    let mut out = vec![Complex::new(0.0, 0.0); to];
    for k in 0..keep {
        out[k] = s[k];
        if k > 0 {
            out[to - k] = s[from - k];
        }
    }
    waveform(&out).into_iter().map(|v| v * to as f64 / from as f64).collect()
}

/// A system's steady-state period: `period` repeated `settle` times and
/// once more, the last one's response.
pub fn steady_state(tick: &mut dyn FnMut(f64) -> f64, period: &[f64], settle: usize) -> Vec<f64> {
    for _ in 0..settle {
        for &x in period {
            tick(x);
        }
    }
    period.iter().map(|&x| tick(x)).collect()
}

/// One steady-state period of input and output at a level.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    /// The input's RMS.
    pub rms: f64,
}

/// A set of periodic records at one rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Records {
    pub rate: f64,
    /// The excited band, Hz.
    pub low_hz: f64,
    pub high_hz: f64,
    pub records: Vec<Record>,
}

impl Records {
    /// Multisine realisations (`realisations` seeds a level) at each RMS
    /// in `levels`, each a period of `len` samples at `rate`, run
    /// through `system` to steady state. `system(input_period)` returns
    /// the output's steady-state period (see [`steady_state`],
    /// [`oversampled`]).
    pub fn measure(
        rate: f64,
        len: usize,
        band: (f64, f64),
        levels: &[f64],
        realisations: u64,
        mut system: impl FnMut(&[f64]) -> Vec<f64>,
    ) -> Self {
        let mut records = Vec::new();
        for &rms in levels {
            for seed in 1..=realisations {
                let input = multisine(len, rate, band.0, band.1, rms, seed);
                let output = system(&input);
                records.push(Record { input, output, rms });
            }
        }
        Self {
            rate,
            low_hz: band.0,
            high_hz: band.1,
            records,
        }
    }

    /// The distinct levels, ascending.
    pub fn levels(&self) -> Vec<f64> {
        let mut l: Vec<f64> = self.records.iter().map(|r| r.rms).collect();
        l.sort_by(f64::total_cmp);
        l.dedup();
        l
    }

    /// The records at `rms`.
    pub fn at(&self, rms: f64) -> impl Iterator<Item = &Record> {
        self.records.iter().filter(move |r| r.rms == rms)
    }
}

/// A system ticked at `factor`× the records' rate on the period
/// band-limit interpolated up, its steady-state period brought back
/// down: the response a perfect anti-aliased measurement at the base
/// rate would read. `make()` gives a fresh system at the high rate;
/// `settle` periods are run before the one returned.
pub fn oversampled(
    factor: usize,
    settle: usize,
    mut make: impl FnMut() -> Box<dyn FnMut(f64) -> f64>,
) -> impl FnMut(&[f64]) -> Vec<f64> {
    move |period: &[f64]| {
        let up = resample(period, period.len() * factor);
        let mut tick = make();
        let out = steady_state(&mut *tick, &up, settle);
        resample(&out, period.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multisine_has_its_rms_and_its_bins() {
        let x = multisine(4096, 48_000.0, 20.0, 20_000.0, 0.1, 7);
        let rms = (x.iter().map(|v| v * v).sum::<f64>() / 4096.0).sqrt();
        assert!((rms - 0.1).abs() < 1e-12);
        let s = spectrum(&x);
        let bins = odd_bins(4096, 48_000.0, 20.0, 20_000.0);
        let a = s[bins[0]].norm();
        assert!(bins.iter().all(|&k| (s[k].norm() - a).abs() < 1e-9 * a));
        assert!((2..2048).step_by(2).all(|k| s[k].norm() < 1e-9 * a));
    }

    #[test]
    fn resampling_a_period_is_exact_for_its_band() {
        let x = multisine(1024, 48_000.0, 20.0, 15_000.0, 0.3, 3);
        let up = resample(&x, 4096);
        let back = resample(&up, 1024);
        let worst = x.iter().zip(&back).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst < 1e-12, "{worst}");
        // The interpolated period passes through the original samples.
        let at = (0..1024).fold(0.0f64, |m, i| m.max((up[4 * i] - x[i]).abs()));
        assert!(at < 1e-12, "{at}");
    }
}
