//! The null test by audibility (DSP.md R16): the residual a model leaves
//! against its reference — `reference − model`, after the model is
//! matched to the reference in level and latency — judged by its
//! noise-to-mask ratio with the reference as the masker, at the playback
//! level.
//!
//! # Models
//!
//! A listener comparing two takes level-matches them and lines them up
//! (a listening test does both before anything else: R21's voicing
//! matches loudness to 0.1 LU and compensates latency), so a gain or a
//! delay is not a difference in sound a model is judged on — the numeric
//! budgets hold those (C2's response, C4's gain, group delay). What is
//! left is the residual, and the question R16 asks of it is whether it
//! is heard under the reference: BS.1387's NMR ([`nmr`](super::nmr))
//! with the residual as the noise ([`Nmr::of_noise`]).
//!
//! The alignment ([`Alignment`]) is one real gain `g` and one delay `τ`
//! for the whole comparison, the least-squares fit of the model's lines
//! to the reference's over the audible band (above DC, up to
//! [`BAND_HZ`]): `τ` maximises `Σ Re(R̄·M·e^{−j2πfτ})` (a coarse search,
//! then a golden-section refinement to well under a nanosecond), `g` is
//! that sum over `Σ|M|²`. Lines may come from periodic runs (a
//! characterisation's tables: several runs fitted jointly, as one level
//! match over the whole session) or from a waveform's spectrum
//! ([`between`]).
//!
//! The verdict ([`Null::inaudible`]): total NMR **and** worst band at
//! most [`INAUDIBLE_DB`], −10 dB. Its basis: an NMR of 0 dB puts the
//! residual at the masked threshold the model predicts in that band, so
//! 0 dB is the edge of audibility for the model's average listener;
//! −10 dB is the margin perceptual coding practice keeps below it for
//! transparency (Brandenburg's NMR measurement, which BS.1387 adopted;
//! the masking offsets themselves vary by several dB between listeners
//! and signals), and the threshold Lehtonen et al. 2012 found for
//! aliasing by listening, which Kahles et al. 2019 adopted for the ANMR
//! ([`AUDIBLE_ANMR_DB`](super::nmr::AUDIBLE_ANMR_DB)). The worst band is
//! held too because the total averages over 109 bands: a residual
//! confined to one band (a resonance off by a little) reads low in total
//! while plainly audible there. A verdict is a model's prediction, never
//! a listening result (R21 records those).
//!
//! # Derivation
//!
//! The NMR is BS.1387's, as implemented in [`nmr`](super::nmr). The
//! alignment is the matched filter for a gain and a delay (the
//! cross-correlation's peak), evaluated on the lines so a delay of any
//! fraction of a sample is exact for periodic data. A waveform's delay
//! is applied as a phase ramp on its zero-padded spectrum, padded past
//! the largest delay searched so nothing wraps; the residual is zeroed
//! within the delay of either end, where the shifted signal has no
//! counterpart. A waveform that starts or stops abruptly rings under a
//! fractional shift: fade stimuli in and out.
//!
//! # Verified
//!
//! `crates/stave-measure/src/perceptual/null.rs::tests`: identical
//! signals sit at the NMR floor; a known error injected at one band and
//! level reads the NMR of that error alone (to 0.05 dB), in that band,
//! and moves dB for dB with its level; a gain of 1.7 dB and a delay of
//! 7.37 samples on the model are fitted to 1e-6 dB and 1e-4 samples and
//! leave the same NMR as without them, while unaligned they read 30 dB
//! worse; the periodic (lines) and waveform paths agree; the residual
//! [`with_residual`] returns (what G5's "listen to the difference"
//! plays) is the injected error to 5 % of its peak, and an alignment
//! [`apply`]d again gives the same verdict.
//!
//! # Cost
//!
//! Two NMR passes' worth of FFTs per comparison, plus the delay search:
//! for lines, `8·f_max·2·max_delay` evaluations over the lines; for a
//! waveform, one inverse FFT for the integer lag.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! As the NMR's: rates from 32 kHz, at least one frame (43 ms) of
//! signal. Delays up to the `max_delay` searched; gains of either sign
//! are fitted but a negative one (an inverted model) is reported, not
//! hidden: the fit's delay search maximises the positive correlation.

use std::f64::consts::TAU;

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

use super::nmr::{Nmr, NmrReport};
use super::Playback;

/// The null test's version (fingerprints pin it).
pub const VERSION: u32 = 1;
/// The inaudible budget: total and worst-band NMR of the null residual
/// at most −10 dB (see the module's *Models*).
pub const INAUDIBLE_DB: f64 = -10.0;
/// The audible band the alignment fits and the residual is judged over:
/// lines above it are left out (BS.1387's bands stop at 18 kHz).
pub const BAND_HZ: f64 = 20_000.0;

/// One line of a comparison: its frequency, the reference's complex
/// amplitude and the model's.
pub type Line = (f64, Complex<f64>, Complex<f64>);

/// A level and latency match: the model, times `gain` and delayed by
/// `delay` seconds, is compared with the reference.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Alignment {
    pub gain: f64,
    pub delay: f64,
}

impl Alignment {
    /// No alignment: unity gain, no delay.
    pub const NONE: Self = Self { gain: 1.0, delay: 0.0 };

    /// The gain in dB (of its magnitude).
    pub fn gain_db(&self) -> f64 {
        20.0 * self.gain.abs().log10()
    }

    /// What the alignment multiplies the model's line at `hz` by (the
    /// convention `x(t) = Re(c·e^{j2πft})`: a delay `τ` is `e^{−j2πfτ}`).
    pub fn factor(&self, hz: f64) -> Complex<f64> {
        let a = -TAU * hz * self.delay;
        Complex::new(self.gain * a.cos(), self.gain * a.sin())
    }

    /// The least-squares gain and delay over `lines` (those above DC up
    /// to [`BAND_HZ`]), the delay searched over `±max_delay` seconds: a
    /// grid at an eighth of the highest line's period, then refined.
    /// [`NONE`](Self::NONE) when the model carries nothing in band.
    pub fn fit(lines: &[Line], max_delay: f64) -> Self {
        let lines: Vec<Line> = in_band(lines).collect();
        let f_max = lines.iter().fold(0.0f64, |m, l| m.max(l.0));
        if f_max == 0.0 {
            return Self::NONE;
        }
        let step = 1.0 / (8.0 * f_max);
        let n = (max_delay.max(0.0) / step).ceil() as i64;
        // The correlation on the grid τ = i·step, i in −n..=n, each line's
        // term turned by a fixed rotation per step.
        let start = -(n as f64) * step;
        let mut terms: Vec<(Complex<f64>, Complex<f64>)> = lines
            .iter()
            .map(|&(hz, r, m)| (r.conj() * m * turn(hz, start), turn(hz, step)))
            .collect();
        let (mut best, mut best_c) = (0.0, f64::NEG_INFINITY);
        for i in -n..=n {
            let mut c = 0.0;
            for (t, rot) in &mut terms {
                c += t.re;
                *t = *t * *rot;
            }
            if c > best_c {
                (best, best_c) = (i as f64 * step, c);
            }
        }
        Self::refine(&lines, best - step, best + step)
    }

    /// The gain and the delay in `lo..=hi` maximising the correlation
    /// (golden section: the correlation is smooth and, within a step of
    /// its peak, unimodal).
    fn refine(lines: &[Line], lo: f64, hi: f64) -> Self {
        let corr = |tau: f64| lines.iter().map(|&(hz, r, m)| (r.conj() * m * turn(hz, tau)).re).sum::<f64>();
        let energy: f64 = lines.iter().map(|l| l.2.norm_sqr()).sum();
        if energy == 0.0 {
            return Self::NONE;
        }
        let phi = (5f64.sqrt() - 1.0) / 2.0;
        let (mut a, mut b) = (lo, hi);
        let (mut c, mut d) = (b - phi * (b - a), a + phi * (b - a));
        let (mut fc, mut fd) = (corr(c), corr(d));
        while b - a > 1e-13 {
            if fc > fd {
                (b, d, fd) = (d, c, fc);
                c = b - phi * (b - a);
                fc = corr(c);
            } else {
                (a, c, fc) = (c, d, fd);
                d = a + phi * (b - a);
                fd = corr(d);
            }
        }
        let delay = 0.5 * (a + b);
        Self {
            gain: corr(delay) / energy,
            delay,
        }
    }
}

/// `e^{−j2πf·τ}`.
fn turn(hz: f64, tau: f64) -> Complex<f64> {
    let a = -TAU * hz * tau;
    Complex::new(a.cos(), a.sin())
}

fn in_band(lines: &[Line]) -> impl Iterator<Item = Line> + '_ {
    lines.iter().copied().filter(|l| l.0 > 0.0 && l.0 <= BAND_HZ)
}

/// A null test's result: the residual's NMR under the reference, after
/// the alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct Null {
    pub nmr: NmrReport,
    pub alignment: Alignment,
}

impl Null {
    /// Within the inaudible budget: total and worst band at most
    /// [`INAUDIBLE_DB`].
    pub fn inaudible(&self) -> bool {
        inaudible(self.nmr.total_db, self.nmr.max_db)
    }
}

/// The verdict on a total and a worst-band NMR, dB.
pub fn inaudible(total_db: f64, max_db: f64) -> bool {
    total_db <= INAUDIBLE_DB && max_db <= INAUDIBLE_DB
}

/// `"inaudible"` or `"AUDIBLE"`, for reports.
pub fn verdict(total_db: f64, max_db: f64) -> &'static str {
    if inaudible(total_db, max_db) {
        "inaudible"
    } else {
        "AUDIBLE"
    }
}

/// The NMR of `reference − aligned` under `reference` (the same length,
/// at `rate`): the residual of an already aligned model.
pub fn residual_nmr(reference: &[f64], aligned: &[f64], rate: f64, playback: Playback) -> NmrReport {
    let residual: Vec<f64> = reference.iter().zip(aligned).map(|(r, a)| r - a).collect();
    Nmr::new(playback).of_noise(reference, &residual, rate)
}

/// The null test of a `model` waveform against a `reference` (the same
/// length, at `rate`): aligned in level and latency (a delay up to
/// `max_delay` seconds either way), then the residual's NMR under the
/// reference. Signals shorter than two NMR frames are zero-padded to
/// two (a short transient measures as itself, then silence).
pub fn between(reference: &[f64], model: &[f64], rate: f64, playback: Playback, max_delay: f64) -> Null {
    with_residual(reference, model, rate, playback, max_delay).null
}

/// A null test with its waveforms: the verdict, the model as aligned
/// and the residual `reference - aligned` (what "listen to the
/// difference" plays). Both as long as the reference, the residual zero
/// within the delay of either end.
#[derive(Clone, Debug, PartialEq)]
pub struct Nulled {
    pub null: Null,
    pub aligned: Vec<f64>,
    pub residual: Vec<f64>,
}

/// The model's spectrum on an FFT long enough for `max_delay`.
struct Padded {
    fft: Fft<f64>,
    n: usize,
    len: usize,
    spectrum: Vec<Complex<f64>>,
}

impl Padded {
    fn new(model: &[f64], rate: f64, max_delay: f64) -> Self {
        let min_len = 3 * super::ear::frame_len(rate) / 2; // two frames at a hop of half a frame
        let len = model.len().max(min_len);
        let lag = (max_delay.max(0.0) * rate).ceil() as usize + 2;
        let n = (len + lag).next_power_of_two().max(2);
        let fft = Fft::new(n);
        let spectrum = spectrum_of(&fft, n, model);
        Self { fft, n, len, spectrum }
    }
}

fn spectrum_of(fft: &Fft<f64>, n: usize, x: &[f64]) -> Vec<Complex<f64>> {
    let mut buf = x.to_vec();
    buf.resize(n, 0.0);
    let mut out = vec![Complex::new(0.0, 0.0); n / 2 + 1];
    fft.forward_real(&buf, &mut out);
    out
}

/// [`between`], keeping the aligned model and the residual.
pub fn with_residual(reference: &[f64], model: &[f64], rate: f64, playback: Playback, max_delay: f64) -> Nulled {
    assert_eq!(reference.len(), model.len(), "the reference and model differ in length");
    let padded = Padded::new(model, rate, max_delay);
    let (n, fft) = (padded.n, &padded.fft);
    let lag = (max_delay.max(0.0) * rate).ceil() as usize + 2;
    let (r, m) = (spectrum_of(fft, n, reference), &padded.spectrum);
    let hz = |k: usize| k as f64 * rate / n as f64;
    // The integer lag: the band-limited cross-correlation's peak within
    // +-lag (the model delayed by `l` samples matches when the
    // correlation of r against m peaks at -l).
    let mut cross: Vec<Complex<f64>> = (0..=n / 2)
        .map(|k| if k > 0 && hz(k) <= BAND_HZ { r[k] * m[k].conj() } else { Complex::new(0.0, 0.0) })
        .collect();
    let mut corr = vec![0.0; n];
    fft.inverse_real(&mut cross, &mut corr);
    let lag = lag.min(n / 2 - 1);
    let best = (0..=lag)
        .map(|l| (l as f64, corr[l]))
        .chain((1..=lag).map(|l| (-(l as f64), corr[n - l])))
        .fold((0.0, f64::NEG_INFINITY), |a, b| if b.1 > a.1 { b } else { a })
        .0;
    let lines: Vec<Line> = (1..=n / 2).filter(|&k| hz(k) <= BAND_HZ).map(|k| (hz(k), r[k], m[k])).collect();
    let alignment = Alignment::refine(&lines, (best - 1.0) / rate, (best + 1.0) / rate);
    finish(reference, model.len(), &padded, alignment, rate, playback)
}

/// The alignment [`between`] finds, alone (one alignment for several
/// channels: fit it on their mix, then [`apply`] it to each).
pub fn align(reference: &[f64], model: &[f64], rate: f64, max_delay: f64) -> Alignment {
    with_residual(reference, model, rate, Playback::default(), max_delay).null.alignment
}

/// The null test of `model` against `reference` under an alignment
/// already fitted (on another channel, or a mix).
pub fn apply(reference: &[f64], model: &[f64], alignment: Alignment, rate: f64, playback: Playback) -> Nulled {
    assert_eq!(reference.len(), model.len(), "the reference and model differ in length");
    let padded = Padded::new(model, rate, alignment.delay.abs());
    finish(reference, model.len(), &padded, alignment, rate, playback)
}

/// Shift and scale the padded model by `alignment`, leave no residual
/// where the shift has no counterpart, and judge the rest.
fn finish(reference: &[f64], real_len: usize, padded: &Padded, alignment: Alignment, rate: f64, playback: Playback) -> Nulled {
    let (n, len, m) = (padded.n, padded.len, &padded.spectrum);
    let hz = |k: usize| k as f64 * rate / n as f64;
    let mut shifted: Vec<Complex<f64>> = (0..=n / 2).map(|k| m[k] * alignment.factor(hz(k))).collect();
    let mut out = vec![0.0; n];
    padded.fft.inverse_real(&mut shifted, &mut out);
    let scale = 1.0 / n as f64;
    let mut reference = reference.to_vec();
    reference.resize(len, 0.0);
    let mut aligned: Vec<f64> = out[..len].iter().map(|v| v * scale).collect();
    // Within the delay of either end the shifted model has no
    // counterpart: no residual there.
    let edge = (alignment.delay.abs() * rate).ceil() as usize + 1;
    for i in (0..edge.min(len)).chain(real_len.saturating_sub(edge)..real_len) {
        aligned[i] = reference[i];
    }
    let nmr = residual_nmr(&reference, &aligned, rate, playback);
    let mut residual: Vec<f64> = reference.iter().zip(&aligned).map(|(r, a)| r - a).collect();
    residual.truncate(real_len);
    aligned.truncate(real_len);
    Nulled {
        null: Null { nmr, alignment },
        aligned,
        residual,
    }
}

/// One period of the waveform `lines` describe, `n` samples at `rate`
/// (`x(t) = Σ Re(c·e^{j2πft})`; each line's frequency a whole number of
/// cycles in the period, `f = k·rate/n`).
pub fn periodic(lines: impl IntoIterator<Item = (f64, Complex<f64>)>, n: usize, rate: f64) -> Vec<f64> {
    let table: Vec<(f64, f64)> = (0..n).map(|i| {
        let a = TAU * i as f64 / n as f64;
        (a.cos(), a.sin())
    }).collect();
    let mut x = vec![0.0; n];
    for (hz, c) in lines {
        let k = (hz * n as f64 / rate).round() as usize % n;
        let mut idx = 0usize;
        for v in x.iter_mut() {
            let (cos, sin) = table[idx];
            *v += c.re * cos - c.im * sin;
            idx += k;
            if idx >= n {
                idx -= n;
            }
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;
    const LEN: usize = 24_000;

    /// A masker of a few partials with a slow envelope (no abrupt ends:
    /// a delayed copy carries no edge), and the same signal delayed by
    /// `delay` samples, exactly (each partial shifted analytically).
    fn signal(delay: f64) -> Vec<f64> {
        let partials = [(220.0, 0.3, 0.1), (440.0, 0.15, 1.3), (1_250.0, 0.08, 2.0), (3_100.0, 0.03, 0.4), (7_700.0, 0.01, 2.9)];
        (0..LEN)
            .map(|i| {
                let t = (i as f64 - delay) / RATE;
                let env = (std::f64::consts::PI * t / (LEN as f64 / RATE)).sin().max(0.0).powi(2);
                env * partials.iter().map(|(f, a, p)| a * (TAU * f * t + p).sin()).sum::<f64>()
            })
            .collect()
    }

    /// An error: a 4 kHz tone at `level` (peak) under the same envelope.
    fn error(level: f64, delay: f64) -> Vec<f64> {
        (0..LEN)
            .map(|i| {
                let t = (i as f64 - delay) / RATE;
                let env = (std::f64::consts::PI * t / (LEN as f64 / RATE)).sin().max(0.0).powi(2);
                level * env * (TAU * 4_000.0 * t).sin()
            })
            .collect()
    }

    fn playback() -> Playback {
        Playback::full_scale(0.5)
    }

    #[test]
    fn identical_signals_sit_at_the_floor() {
        let x = signal(0.0);
        let null = between(&x, &x, RATE, playback(), 1e-3);
        assert!(null.nmr.total_db < -100.0 && null.nmr.max_db < -90.0, "{:?}", (null.nmr.total_db, null.nmr.max_db));
        assert!((null.alignment.gain - 1.0).abs() < 1e-9 && null.alignment.delay.abs() < 1e-9, "{:?}", null.alignment);
        assert!(null.inaudible());
    }

    /// A known error at 4 kHz: the null reads exactly that error's NMR
    /// under the reference, worst in its band, and dB for dB with its
    /// level; −30 dB re the masker's 4 kHz partial region is audible,
    /// far lower is not.
    #[test]
    fn an_injected_error_reads_its_own_nmr() {
        let x = signal(0.0);
        let nmr = Nmr::new(playback());
        for level in [1e-3, 1e-4, 1e-5] {
            let e = error(level, 0.0);
            let model: Vec<f64> = x.iter().zip(&e).map(|(a, b)| a - b).collect();
            let null = between(&x, &model, RATE, playback(), 1e-3);
            let expected = nmr.of_noise(&x, &e, RATE);
            assert!((null.nmr.total_db - expected.total_db).abs() < 0.05, "{level}: {} vs {}", null.nmr.total_db, expected.total_db);
            assert!((null.nmr.max_db - expected.max_db).abs() < 0.05, "{level}: {} vs {}", null.nmr.max_db, expected.max_db);
            assert!((null.nmr.max_hz - 4_000.0).abs() < 250.0, "{}", null.nmr.max_hz);
        }
        let at = |level: f64| {
            let e = error(level, 0.0);
            let model: Vec<f64> = x.iter().zip(&e).map(|(a, b)| a - b).collect();
            between(&x, &model, RATE, playback(), 1e-3).nmr
        };
        let (loud, quiet) = (at(1e-3), at(1e-4));
        assert!((loud.max_db - quiet.max_db - 20.0).abs() < 0.1, "{} {}", loud.max_db, quiet.max_db);
        assert!((loud.total_db - quiet.total_db - 20.0).abs() < 0.5, "{} {}", loud.total_db, quiet.total_db);
        assert!(!inaudible(loud.total_db, loud.max_db), "{} {}", loud.total_db, loud.max_db);
        let faint = at(1e-6);
        assert!(inaudible(faint.total_db, faint.max_db), "{} {}", faint.total_db, faint.max_db);
    }

    /// A gain and a fractional delay on the model are fitted and leave
    /// the injected error's NMR; unaligned, the same model reads far
    /// worse.
    #[test]
    fn gain_and_latency_are_aligned_out() {
        let (gain, delay) = (10f64.powf(1.7 / 20.0), 7.37);
        let x = signal(0.0);
        let e = error(1e-4, 0.0);
        let plain: Vec<f64> = x.iter().zip(&e).map(|(a, b)| a - b).collect();
        let expected = between(&x, &plain, RATE, playback(), 1e-3).nmr;
        let ed = error(1e-4, delay);
        let model: Vec<f64> = signal(delay).iter().zip(&ed).map(|(a, b)| (a - b) / gain).collect();
        let null = between(&x, &model, RATE, playback(), 1e-3);
        assert!((null.alignment.gain_db() - 1.7).abs() < 1e-4, "{:?}", null.alignment);
        assert!((null.alignment.delay * RATE + delay).abs() < 1e-3, "{:?}", null.alignment.delay * RATE);
        assert!((null.nmr.total_db - expected.total_db).abs() < 0.2, "{} vs {}", null.nmr.total_db, expected.total_db);
        assert!((null.nmr.max_db - expected.max_db).abs() < 0.2, "{} vs {}", null.nmr.max_db, expected.max_db);
        let unaligned = residual_nmr(&x, &model, RATE, playback());
        assert!(unaligned.total_db > expected.total_db + 30.0, "{} vs {}", unaligned.total_db, expected.total_db);
    }

    /// The lines path: periodic data, aligned on its lines, the delay
    /// exact to any fraction of a sample.
    #[test]
    fn lines_align_exactly() {
        let (n, k0) = (2_400usize, 3u32); // 50 ms: 20 Hz lines
        let lines: Vec<(f64, Complex<f64>)> = (0..40)
            .map(|j| {
                let k = k0 + 25 * j;
                let a = 0.2 / (1.0 + j as f64);
                (f64::from(k) * RATE / n as f64, Complex::new(a * (1.1 * j as f64).cos(), a * (1.1 * j as f64).sin()))
            })
            .collect();
        let truth = Alignment { gain: 0.8, delay: 2.31 / RATE };
        let joined: Vec<Line> = lines
            .iter()
            .map(|&(hz, c)| {
                let back = truth.factor(hz);
                // The model is the reference with the inverse alignment applied.
                (hz, c, crate::suite::divide(c, back))
            })
            .collect();
        let fitted = Alignment::fit(&joined, 1e-3);
        assert!((fitted.gain - 0.8).abs() < 1e-9 && (fitted.delay - truth.delay).abs() < 1e-11, "{fitted:?}");
        // Aligned, the residual is nil.
        let r = periodic(joined.iter().map(|l| (l.0, l.1)), n, RATE);
        let a = periodic(joined.iter().map(|l| (l.0, l.2 * fitted.factor(l.0))), n, RATE);
        let len = 12_000;
        let nmr = residual_nmr(&super::super::tiled(&r, len), &super::super::tiled(&a, len), RATE, playback());
        assert!(nmr.total_db < -100.0, "{}", nmr.total_db);
    }

    /// The residual a null plays back is the one it judges: with the
    /// model delayed and scaled, `with_residual` returns the injected
    /// error as its residual, and an alignment fitted once and applied
    /// again gives the same verdict.
    #[test]
    fn the_residual_is_the_injected_error() {
        let (gain, delay) = (10f64.powf(-2.3 / 20.0), 3.25);
        let x = signal(0.0);
        let e = error(1e-3, 0.0);
        let ed = error(1e-3, delay);
        let model: Vec<f64> = signal(delay).iter().zip(&ed).map(|(a, b)| (a - b) * gain).collect();
        let nulled = with_residual(&x, &model, RATE, playback(), 1e-3);
        assert_eq!(nulled.null, between(&x, &model, RATE, playback(), 1e-3));
        assert_eq!((nulled.residual.len(), nulled.aligned.len()), (LEN, LEN));
        let worst = nulled.residual.iter().zip(&e).map(|(r, e)| (r - e).abs()).fold(0.0, f64::max);
        assert!(worst < 0.05 * 1e-3, "residual off the injected error (peak 1e-3) by {worst}");
        let again = apply(&x, &model, nulled.null.alignment, RATE, playback());
        assert!((again.null.nmr.total_db - nulled.null.nmr.total_db).abs() < 1e-6);
        assert_eq!(align(&x, &model, RATE, 1e-3), nulled.null.alignment);
    }
}
