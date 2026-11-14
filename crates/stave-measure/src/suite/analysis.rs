//! The analysers: what the suite reads from a model's (or a reference's)
//! output over whole periods.
//!
//! Complex amplitudes follow one convention throughout: `c` at bin `k`
//! is `(2/N)·Σ x[n]·e^{−j2πkn/N}` over one period starting at a whole
//! period (DC: the mean), so `A·sin(2πkt/T + φ)` reads `A·e^{j(φ−π/2)}`.

use super::excitation::{Excitation, Tone};
use super::multisine::Multisine;
use super::period::{products, Period};
use super::{db, divide, wrap, Complex};
use crate::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

/// A period's complex amplitudes at chosen bins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spectrum {
    pub bins: Vec<(u32, Complex<f64>)>,
}

impl Spectrum {
    pub fn get(&self, bin: u32) -> Option<Complex<f64>> {
        self.bins.iter().find(|(k, _)| *k == bin).map(|(_, c)| *c)
    }

    pub fn magnitude(&self, bin: u32) -> Option<f64> {
        self.get(bin).map(Complex::norm)
    }
}

/// One period's DFT, by a twiddle table (exact indices, no recurrence).
struct Dft {
    cos: Vec<f64>,
    sin: Vec<f64>,
}

impl Dft {
    fn new(n: usize) -> Self {
        let w = std::f64::consts::TAU / n as f64;
        let (cos, sin) = (0..n).map(|i| ((w * i as f64).cos(), (w * i as f64).sin())).unzip();
        Self { cos, sin }
    }

    fn at(&self, x: &[f64], bin: u32) -> Complex<f64> {
        let n = x.len();
        let step = bin as usize % n;
        let (mut re, mut im, mut idx) = (0.0, 0.0, 0usize);
        for &v in x {
            re += v * self.cos[idx];
            im -= v * self.sin[idx];
            idx += step;
            if idx >= n {
                idx -= n;
            }
        }
        let scale = if bin == 0 { 1.0 } else { 2.0 } / n as f64;
        Complex::new(re * scale, im * scale)
    }
}

/// The complex amplitudes at `wanted` of `x`, one period at `rate`
/// starting at a whole period. Bins at or above Nyquist are left out:
/// they are not measurable at this rate.
pub fn bins(x: &[f64], period: Period, rate: f64, wanted: &[u32]) -> Spectrum {
    let n = period.samples(rate);
    assert_eq!(x.len(), n, "one period at {rate} Hz is {n} samples");
    let dft = Dft::new(n);
    Spectrum {
        bins: wanted
            .iter()
            .filter(|&&k| 2 * (k as usize) < n)
            .map(|&k| (k, dft.at(x, k)))
            .collect(),
    }
}

/// Every bin of `x`'s period in the audible band (20 Hz – 20 kHz, below
/// Nyquist), with its power `|c|²/2`.
pub fn audible(x: &[f64], period: Period, rate: f64) -> Vec<(u32, f64)> {
    let n = period.samples(rate);
    assert_eq!(x.len(), n);
    let dft = Dft::new(n);
    let lo = (AUDIBLE_MIN_HZ * period.seconds()).ceil().max(1.0) as u32;
    let hi = (AUDIBLE_MAX_HZ * period.seconds()).floor() as u32;
    (lo..=hi)
        .filter(|&k| 2 * (k as usize) < n)
        .map(|k| (k, dft.at(x, k).norm_sqr() / 2.0))
        .collect()
}

/// A tone's complex amplitude: `A·e^{j(φ−π/2)}`.
pub fn phasor(tone: &Tone) -> Complex<f64> {
    let p = tone.phase - std::f64::consts::FRAC_PI_2;
    Complex::new(tone.amplitude * p.cos(), tone.amplitude * p.sin())
}

/// The fundamental and harmonics 2..=count of a tone on `bin`.
#[derive(Clone, Debug, PartialEq)]
pub struct Harmonics {
    pub bin: u32,
    /// Index `h − 1` holds harmonic `h`; `None` where not measured.
    pub amplitudes: Vec<Option<Complex<f64>>>,
}

impl Harmonics {
    pub fn of(spectrum: &Spectrum, bin: u32, count: u32) -> Self {
        Self {
            bin,
            amplitudes: (1..=count).map(|h| spectrum.get(h * bin)).collect(),
        }
    }

    pub fn get(&self, h: u32) -> Option<Complex<f64>> {
        self.amplitudes.get(h as usize - 1).copied().flatten()
    }

    pub fn fundamental(&self) -> Complex<f64> {
        self.get(1).expect("the fundamental is measured")
    }

    /// Harmonic `h`'s level re the fundamental, in dB.
    pub fn dbc(&self, h: u32) -> Option<f64> {
        Some(db(self.get(h)?.norm() / self.fundamental().norm()))
    }

    /// Harmonic `h`'s phase relative to the fundamental's:
    /// `arg c_h − h·arg c_1`, wrapped. The waveform's shape, unchanged by
    /// a delay.
    pub fn shape_phase(&self, h: u32) -> Option<f64> {
        Some(wrap(self.get(h)?.arg() - f64::from(h) * self.fundamental().arg()))
    }

    /// THD over the measured harmonics 2…, as a ratio.
    pub fn thd(&self) -> f64 {
        let power: f64 = (2..=self.amplitudes.len() as u32)
            .filter_map(|h| self.get(h))
            .map(Complex::norm_sqr)
            .sum();
        power.sqrt() / self.fundamental().norm()
    }
}

/// The fundamental's complex gain: output over input amplitude
/// (compression in its magnitude, the phase shift in its argument).
pub fn gain(input: &Tone, fundamental: Complex<f64>) -> Complex<f64> {
    divide(fundamental, phasor(input))
}

/// THD+N: the audible band's power, DC and the fundamental excluded,
/// over the fundamental's, as an amplitude ratio.
pub fn thd_n(x: &[f64], period: Period, rate: f64, bin: u32) -> f64 {
    let band = audible(x, period, rate);
    let fundamental = band.iter().find(|b| b.0 == bin).map_or(0.0, |b| b.1);
    let rest: f64 = band.iter().filter(|b| b.0 != bin).map(|b| b.1).sum();
    (rest / fundamental).sqrt()
}

/// Aliasing (and noise) on empty bins: the audible band's power off
/// every harmonic of `bin`, over the fundamental's, as an amplitude
/// ratio. With a collision-free bin
/// ([`tone_bin`](super::period::tone_bin)) a folded harmonic lands here,
/// never on a harmonic.
pub fn aliasing(x: &[f64], period: Period, rate: f64, bin: u32) -> f64 {
    let band = audible(x, period, rate);
    let fundamental = band.iter().find(|b| b.0 == bin).map_or(0.0, |b| b.1);
    let rest: f64 = band.iter().filter(|b| b.0 % bin != 0).map(|b| b.1).sum();
    (rest / fundamental).sqrt()
}

/// Kahles et al.'s ANMR of a tone run's period (DSP.md C10, R19): the
/// inharmonic remainder (aliases, noise) under the mask its harmonics of
/// `bin` set, A-weighted, in dB — audible above −10 dB. The period is
/// played with its peak (about its mean) at full scale, 92 dB SPL
/// ([`aliasing::anmr`](crate::aliasing::anmr)'s rule).
pub fn anmr(x: &[f64], period: Period, rate: f64, bin: u32) -> f64 {
    let n = x.len();
    let mean = x.iter().sum::<f64>() / n as f64;
    let harmonics: Vec<u32> = (1..).map(|h| h * bin).take_while(|&k| 2 * (k as usize) < n).collect();
    let masker = waveform(&bins(x, period, rate, &harmonics), period, rate);
    let noise: Vec<f64> = x.iter().zip(&masker).map(|(v, m)| v - mean - m).collect();
    let peak = x.iter().fold(0.0f64, |p, v| p.max((v - mean).abs())).max(1e-30);
    let len = (rate / 2.0) as usize;
    let (masker, noise) = (crate::perceptual::tiled(&masker, len), crate::perceptual::tiled(&noise, len));
    crate::perceptual::nmr::Nmr::anmr(crate::perceptual::Playback::full_scale(peak), &masker, &noise, rate).total_db
}

/// One period at `rate` of the waveform `spectrum`'s bins describe
/// (this module's convention: `x(t) = c₀ + Σ Re(c_k·e^{j2πkt/T})`). A
/// table holds only measured bins, so this is the signal those bins
/// carry: what a perceptual metric compares when it compares tables.
pub fn waveform(spectrum: &Spectrum, period: Period, rate: f64) -> Vec<f64> {
    let n = period.samples(rate);
    let w = std::f64::consts::TAU / n as f64;
    let mut x = vec![0.0; n];
    for &(k, c) in &spectrum.bins {
        if k == 0 {
            x.iter_mut().for_each(|v| *v += c.re);
            continue;
        }
        let step = k as usize % n;
        let mut idx = 0usize;
        for v in x.iter_mut() {
            let a = w * idx as f64;
            *v += c.re * a.cos() - c.im * a.sin();
            idx += step;
            if idx >= n {
                idx -= n;
            }
        }
    }
    x
}

/// Periodic steady state: the RMS change between the last two periods
/// over the last period's RMS about its mean. Zero once settled.
pub fn steady_state(previous: &[f64], last: &[f64]) -> f64 {
    let n = last.len() as f64;
    let mean = last.iter().sum::<f64>() / n;
    let change: f64 = previous.iter().zip(last).map(|(a, b)| (b - a) * (b - a)).sum();
    let signal: f64 = last.iter().map(|b| (b - mean) * (b - mean)).sum();
    (change / signal.max(f64::MIN_POSITIVE)).sqrt()
}

/// The best linear approximation from an odd-odd multisine: per excited
/// bin, the output over the input.
#[derive(Clone, Debug, PartialEq)]
pub struct Bla {
    pub period: Period,
    pub bins: Vec<u32>,
    pub response: Vec<Complex<f64>>,
}

impl Bla {
    /// From the excitation and the output's spectrum (bins above the
    /// output's Nyquist are left out).
    pub fn of(excitation: &Excitation, output: &Spectrum) -> Self {
        let (bins, response) = excitation
            .tones
            .iter()
            .filter_map(|t| Some((t.bin, divide(output.get(t.bin)?, phasor(t)))))
            .unzip();
        Self {
            period: excitation.period,
            bins,
            response,
        }
    }

    pub fn hz(&self) -> Vec<f64> {
        self.bins.iter().map(|&k| self.period.hz(k)).collect()
    }

    pub fn magnitude_db(&self) -> Vec<f64> {
        self.response.iter().map(|c| db(c.norm())).collect()
    }

    /// Radians, unwrapped along frequency.
    pub fn phase(&self) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::with_capacity(self.response.len());
        for c in &self.response {
            let p = c.arg();
            let p = match out.last() {
                Some(&last) => last + wrap(p - last),
                None => p,
            };
            out.push(p);
        }
        out
    }

    /// Seconds, between neighbouring excited bins (`−Δφ/Δω`), at their
    /// geometric-mean frequency.
    pub fn group_delay(&self) -> Vec<(f64, f64)> {
        let phase = self.phase();
        let hz = self.hz();
        (1..phase.len())
            .map(|i| {
                let dw = std::f64::consts::TAU * (hz[i] - hz[i - 1]);
                ((hz[i] * hz[i - 1]).sqrt(), -(phase[i] - phase[i - 1]) / dw)
            })
            .collect()
    }
}

/// Distortion on a multisine's detection bins, RMS over the excited
/// output's RMS, as amplitude ratios: `(even order, odd order)`.
pub fn detection(multisine: &Multisine, output: &Spectrum) -> (f64, f64) {
    let power = |bins: &[u32]| -> f64 {
        bins.iter()
            .filter_map(|&k| output.get(k))
            .map(Complex::norm_sqr)
            .sum()
    };
    let excited = power(&multisine.bins);
    (
        (power(&multisine.even_detection()) / excited).sqrt(),
        (power(&multisine.odd_detection()) / excited).sqrt(),
    )
}

/// A two-tone run's intermodulation products to `order`, each
/// `(m, n, bin, complex amplitude)`, the tones themselves excluded.
pub fn intermodulation(
    low: u32,
    high: u32,
    order: u32,
    output: &Spectrum,
) -> Vec<(i32, i32, u32, Complex<f64>)> {
    products(low, high, order)
        .into_iter()
        .filter(|&(m, n, _)| !((m, n) == (1, 0) || (m, n) == (0, 1)))
        .filter_map(|(m, n, k)| Some((m, n, k as u32, output.get(k as u32)?)))
        .collect()
}

/// SMPTE RP120 IMD: the RMS of the sidebands `high ± n·low` (n = 1…6)
/// over the high tone's amplitude.
pub fn smpte_imd(low: u32, high: u32, output: &Spectrum) -> f64 {
    let carrier = output.magnitude(high).unwrap_or(0.0);
    let side: f64 = (1..=6u32)
        .flat_map(|n| [high + n * low, high.saturating_sub(n * low)])
        .filter_map(|k| output.get(k))
        .map(Complex::norm_sqr)
        .sum();
    side.sqrt() / carrier
}

/// After a burst: per measured period, the output's mean (bias shift)
/// and a probe tone's level in dB re its level in the first period
/// (blocking, gain recovery).
#[derive(Clone, Debug, PartialEq)]
pub struct Recovery {
    pub dc: Vec<f64>,
    pub probe_db: Vec<f64>,
}

impl Recovery {
    pub fn of(periods: &[Spectrum], probe: u32) -> Self {
        let dc = periods.iter().map(|s| s.get(0).map_or(0.0, |c| c.re)).collect();
        let level: Vec<f64> = periods
            .iter()
            .map(|s| s.magnitude(probe).unwrap_or(0.0))
            .collect();
        let first = level.first().copied().unwrap_or(1.0);
        Self {
            dc,
            probe_db: level.iter().map(|l| db(l / first)).collect(),
        }
    }

    /// The bias shift's 1/e recovery time, seconds: from period `from`
    /// (the first after a level step down), how long the mean takes to
    /// fall to 1/e of its shift there from `base` (the settled mean
    /// before the step) — an exponential's time constant whichever
    /// period it is read from. Between periods the shift is taken as
    /// exponential (linear when it crosses zero: an overshoot). `None`
    /// when the shift at `from` is under `min_shift` (nothing to
    /// recover); infinite when it has not recovered by the last period.
    pub fn time_constant(&self, base: f64, from: usize, period_seconds: f64, min_shift: f64) -> Option<f64> {
        let shift = |j: usize| self.dc[j] - base;
        let d0 = self.dc.get(from)? - base;
        if d0.is_nan() || d0.abs() < min_shift {
            return None;
        }
        let target = d0 / std::f64::consts::E;
        for j in from + 1..self.dc.len() {
            let (a, b) = (shift(j - 1), shift(j));
            // Past the target: on the far side of it, toward zero or beyond.
            if b * d0.signum() <= target * d0.signum() {
                let fraction = if a * b > 0.0 {
                    (a / target).ln() / (a / b).ln()
                } else {
                    (a - target) / (a - b)
                };
                return Some(((j - 1 - from) as f64 + fraction) * period_seconds);
            }
        }
        Some(f64::INFINITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A period's bins, back to the waveform: exact for a line spectrum.
    #[test]
    fn waveforms_round_trip_through_bins() {
        let period = Period::units(6);
        let rate = 48_000.0;
        let n = period.samples(rate);
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / rate;
                0.3 + 0.5 * (std::f64::consts::TAU * 50.0 * 3.0 * t + 0.4).sin() + 0.1 * (std::f64::consts::TAU * 50.0 * 7.0 * t).cos()
            })
            .collect();
        let s = bins(&x, period, rate, &[0, 3, 7]);
        let y = waveform(&s, period, rate);
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-12));
    }

    /// A clean tone's ANMR sits at the floor; a hard clip's at the host
    /// rate does not.
    #[test]
    fn anmr_of_a_period() {
        let period = Period::units(6);
        let rate = 48_000.0;
        let bin = super::super::period::tone_bin(period, 3_000.0, 15, &[rate]).unwrap();
        let n = period.samples(rate);
        let tone: Vec<f64> = (0..n).map(|i| (std::f64::consts::TAU * f64::from(bin) * i as f64 / n as f64).sin()).collect();
        assert!(anmr(&tone, period, rate, bin) < -60.0);
        let clipped: Vec<f64> = tone.iter().map(|v| (4.0 * v).clamp(-1.0, 1.0)).collect();
        assert!(anmr(&clipped, period, rate, bin) > -10.0);
    }
    use crate::suite::period::tone_bin;

    fn process(e: &Excitation, rate: f64, from: f64, f: impl Fn(f64) -> f64) -> Vec<f64> {
        let n = e.period.samples(rate);
        (0..n).map(|i| f(e.at(from + i as f64 / rate))).collect()
    }

    fn close(a: Complex<f64>, b: Complex<f64>, tol: f64) -> bool {
        (a - b).norm() <= tol
    }

    /// y = x + a₂x² + a₃x³ on A·sin θ: DC a₂A²/2, H1 A + 3a₃A³/4, H2
    /// (a₂A²/2)·e^{−jπ}, H3 (a₃A³/4)·e^{jπ/2}, nothing above.
    #[test]
    fn harmonics_of_a_polynomial_are_its_closed_form() {
        let (a2, a3, amp) = (0.2, -0.1, 0.8);
        let p = Period::units(6);
        let bin = tone_bin(p, 1_000.0, 15, &[48_000.0]).unwrap();
        let e = Excitation::sine(p, bin, amp);
        let rate = 48_000.0;
        let y = process(&e, rate, p.seconds(), |x| x + a2 * x * x + a3 * x * x * x);
        let want: Vec<u32> = (0..=15).map(|h| h * bin).collect();
        let s = bins(&y, p, rate, &want);
        let h = Harmonics::of(&s, bin, 15);
        let polar = |r: f64, phi: f64| Complex::new(r * phi.cos(), r * phi.sin());
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!((s.get(0).unwrap().re - a2 * amp * amp / 2.0).abs() < 1e-12);
        assert!(close(h.fundamental(), polar(amp + 0.75 * a3 * amp.powi(3), -half_pi), 1e-12));
        assert!(close(h.get(2).unwrap(), polar(a2 * amp * amp / 2.0, -2.0 * half_pi), 1e-12));
        assert!(close(h.get(3).unwrap(), polar(a3.abs() * amp.powi(3) / 4.0, -half_pi), 1e-12));
        for k in 4..=15 {
            assert!(h.get(k).unwrap().norm() < 1e-13);
        }
        let g = gain(&e.tones[0], h.fundamental());
        assert!((g.norm() - (1.0 + 0.75 * a3 * amp * amp)).abs() < 1e-12 && g.arg().abs() < 1e-12);
        // A delay moves arg c_h by h·arg c_1: the shape phase stays.
        let late = process(&e, rate, p.seconds() + 3.0 / rate, |x| x + a2 * x * x + a3 * x * x * x);
        let h2 = Harmonics::of(&bins(&late, p, rate, &want), bin, 15);
        for k in [2, 3] {
            assert!(wrap(h2.shape_phase(k).unwrap() - h.shape_phase(k).unwrap()).abs() < 1e-9);
        }
        assert!((h.thd() - (h.get(2).unwrap().norm_sqr() + h.get(3).unwrap().norm_sqr()).sqrt() / h.fundamental().norm()).abs() < 1e-15);
        let thdn = thd_n(&y, p, rate, bin);
        assert!((thdn - h.thd()).abs() < 1e-9, "{thdn} vs {}", h.thd());
        assert!(aliasing(&y, p, rate, bin) < 1e-12);
    }

    /// Bins at or above a rate's Nyquist are not measured; an inharmonic
    /// component reads as aliasing and in THD+N.
    #[test]
    fn empty_bins_read_aliasing() {
        let p = Period::units(6);
        let rate = 48_000.0;
        let e = Excitation::sine(p, 21, 1.0).with_tone(Tone::new(1e-3, 333));
        let y = process(&e, rate, 0.0, |x| x);
        assert!((aliasing(&y, p, rate, 21) - 1e-3).abs() < 1e-12);
        assert!((thd_n(&y, p, rate, 21) - 1e-3).abs() < 1e-12);
        assert!(bins(&y, p, rate, &[479, 480, 481]).bins.len() == 1);
    }

    /// The BLA of a known filter is its response at each bin; a pure
    /// delay reads as its group delay; a square law fills only the even
    /// detection bins and a cubic only the odd ones.
    #[test]
    fn the_bla_and_detection_bins_of_known_systems() {
        let p = Period::units(15);
        let rate = 48_000.0;
        let m = Multisine::odd_odd(p, 20.0, 20_000.0, 3.0);
        let e = m.excitation(0.1, crate::suite::multisine::Phases::Random { seed: 3 });
        // y[n] = 0.3 x[n] + 0.2 x[n−1] + 0.5 y[n−1], run 3 periods.
        let n = p.samples(rate);
        let (mut x1, mut y1) = (0.0, 0.0);
        let out: Vec<f64> = (0..3 * n)
            .map(|i| {
                let x = e.at(i as f64 / rate);
                let y = 0.3 * x + 0.2 * x1 + 0.5 * y1;
                (x1, y1) = (x, y);
                y
            })
            .collect();
        let s = bins(&out[2 * n..], p, rate, &m.read_bins());
        let bla = Bla::of(&e, &s);
        for (k, g) in bla.bins.iter().zip(&bla.response) {
            let w = std::f64::consts::TAU * f64::from(*k) / n as f64;
            let z = Complex::new(w.cos(), -w.sin());
            let want = divide(
                Complex::new(0.3, 0.0) + z.scale(0.2),
                Complex::new(1.0, 0.0) - z.scale(0.5),
            );
            assert!(close(*g, want, 1e-9), "bin {k}: {g:?} vs {want:?}");
        }
        let d = 7usize;
        let delayed: Vec<f64> = (0..n)
            .map(|i| e.at((i + 2 * n) as f64 / rate - d as f64 / rate))
            .collect();
        let gd = Bla::of(&e, &bins(&delayed, p, rate, &m.read_bins())).group_delay();
        assert!(gd.iter().all(|(_, s)| (s - d as f64 / rate).abs() < 1e-9));

        let square: Vec<f64> = (0..n).map(|i| e.at(i as f64 / rate)).map(|x| x + 0.5 * x * x).collect();
        let (even, odd) = detection(&m, &bins(&square, p, rate, &m.read_bins()));
        assert!(even > 1e-4 && odd < 1e-12, "{even} {odd}");
        let cubic: Vec<f64> = (0..n).map(|i| e.at(i as f64 / rate)).map(|x| x + 5.0 * x * x * x).collect();
        let (even, odd) = detection(&m, &bins(&cubic, p, rate, &m.read_bins()));
        assert!(odd > 1e-4 && even < 1e-12, "{even} {odd}");
    }

    /// A square law on two tones: the difference and sum products are
    /// a₂·A₁·A₂, the second harmonics a₂A²/2; SMPTE IMD reads the sidebands.
    #[test]
    fn intermodulation_of_a_square_law() {
        let p = Period::units(15);
        let rate = 48_000.0;
        let e = Excitation::smpte(p, 1.0).unwrap();
        let (lo, hi) = (e.tones[0].bin, e.tones[1].bin);
        let a2 = 0.1;
        let y = process(&e, rate, 0.0, |x| x + a2 * x * x);
        let want: Vec<u32> = products(lo, hi, 4).iter().map(|q| q.2 as u32).collect();
        let s = bins(&y, p, rate, &want);
        let imd = intermodulation(lo, hi, 4, &s);
        let (a, b) = (e.tones[0].amplitude, e.tones[1].amplitude);
        let at = |m: i32, n: i32| imd.iter().find(|q| (q.0, q.1) == (m, n)).unwrap().3.norm();
        assert!((at(1, 1) - a2 * a * b).abs() < 1e-12);
        assert!((at(-1, 1) - a2 * a * b).abs() < 1e-12);
        assert!((at(2, 0) - a2 * a * a / 2.0).abs() < 1e-12);
        assert!(at(3, 0) < 1e-12);
        let smpte = smpte_imd(lo, hi, &s);
        assert!((smpte - (2.0f64).sqrt() * a2 * a * b / b).abs() < 1e-9, "{smpte}");
    }

    #[test]
    fn recovery_reads_bias_and_probe_per_period() {
        let p = Period::units(3);
        let rate = 48_000.0;
        let n = p.samples(rate);
        let periods: Vec<Spectrum> = (0..4)
            .map(|j| {
                let level = [1.0, 0.5, 0.75, 1.0][j];
                let shift = [0.0, -0.2, -0.1, 0.0][j];
                let e = Excitation::sine(p, 23, level).with_dc(shift);
                let x = process(&e, rate, 0.0, |x| x);
                bins(&x, p, rate, &[0, 23])
            })
            .collect();
        let r = Recovery::of(&periods, 23);
        assert!((r.dc[1] + 0.2).abs() < 1e-12 && (r.probe_db[1] - db(0.5)).abs() < 1e-9);
        // 1/e of −0.2 lies between −0.1 and +0.05 (an overshoot): a linear step.
        let overshoot = Recovery {
            dc: vec![0.0, -0.2, -0.1, 0.05],
            probe_db: vec![0.0; 4],
        };
        let tau = overshoot.time_constant(0.0, 1, 0.01, 1e-6).unwrap();
        let want = (1.0 + (0.1 - 0.2 / std::f64::consts::E) / 0.15) * 0.01;
        assert!((tau - want).abs() < 1e-12, "{tau} vs {want}");
        assert_eq!(r.time_constant(0.0, 3, 0.01, 1e-6), None);
        // An exponential's time constant, read from any period.
        let decay = Recovery {
            dc: (0..8).map(|j| 0.3 + 0.7 * (-0.5 * j as f64).exp()).collect(),
            probe_db: vec![0.0; 8],
        };
        for from in 0..3 {
            let tau = decay.time_constant(0.3, from, 0.01, 1e-6).unwrap();
            assert!((tau - 0.02).abs() < 1e-12, "from {from}: {tau}");
        }
        assert_eq!(decay.time_constant(-1.0, 0, 0.01, 1e-6), Some(f64::INFINITY));
        assert_eq!(n, 480);
        let prev: Vec<f64> = vec![0.0; 4];
        assert_eq!(steady_state(&[1.0, -1.0, 1.0, -1.0], &[1.0, -1.0, 1.0, -1.0]), 0.0);
        assert!(steady_state(&prev, &[1.0, -1.0, 1.0, -1.0]) > 0.9);
    }
}
