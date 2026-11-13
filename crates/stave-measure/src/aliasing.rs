//! Aliasing (DSP.md A4): the inharmonic energy of a stepped tone through
//! a stage, or of an oscillator stepped across its range, in dB below
//! the fundamental, with the harmonics and the aliases separated by
//! frequency.
//!
//! Each step is a bin-centred tone at an **odd** bin of a power-of-two
//! window, so the output of a steady system repeats exactly every window
//! and its spectrum needs no window function: the true harmonics `k·f₀`
//! below Nyquist land on the bins `k·m`, and everything a nonlinearity
//! folds back from above Nyquist lands on other bins (a fold of
//! `k·m` is `−k·m mod n`, a multiple of `m` only when `n` divides a
//! multiple of the odd `m`, i.e. never below the window's length in
//! harmonics). So the classification is exact: a bin that is a multiple
//! of the carrier is a harmonic, any other bin (above DC) is inharmonic —
//! an alias, or noise, or a subharmonic, all of which a clean stage
//! does not make.
//!
//! Beside the figure in dB below the fundamental, each step carries Sato
//! & Smith's aliasing-to-signal ratio ([`AliasPoint::asr`]; "Aliasing
//! reduction in neural amp modeling by smoothing activations", DAFx
//! 2025): the same inharmonic energy over the *whole* harmonic energy,
//! fundamental included, as a linear ratio, the scale neural-model
//! papers quote beside their error-to-signal ratio. The two differ by
//! the harmonics: a stage that distorts heavily has a lower ASR than its
//! dB-re-fundamental figure suggests.
//!
//! The audibility figure is the **A-weighted noise-to-mask ratio**
//! ([`anmr`]; Kahles, Esqueda & Välimäki 2019): the inharmonic bins in
//! the audible band as the noise, the harmonics as the masker, through
//! BS.1387's ear model with A-weighting ([`Nmr::anmr`]). Aliasing is
//! audible above −10 dB ([`AUDIBLE_ANMR_DB`], Lehtonen et al. 2012).
//! Absolute level matters to the ear model, so the capture is played
//! with its peak at full scale, 92 dB SPL: as loud as a listener
//! would hear it, never quieter. [`SteppedTone::piano`] steps a tone
//! over the piano's range (27.5 Hz – 4186 Hz), where Kahles et al. and
//! the measurement design read it.
//!
//! ```
//! use stave_measure::aliasing::SteppedTone;
//!
//! // A hard clipper at the base rate folds plenty.
//! let sweep = SteppedTone { low_hz: 1_000.0, high_hz: 8_000.0, steps: 4, ..SteppedTone::default() };
//! let report = sweep.through(|| |x: f32| (4.0 * x).clamp(-1.0, 1.0));
//! assert!(report.worst().inharmonic_db > -40.0);
//! // A gain makes no harmonics and folds nothing.
//! let clean = sweep.through(|| |x: f32| 0.5 * x);
//! assert!(clean.worst().inharmonic_db < -120.0);
//! ```

use crate::perceptual::nmr::{Nmr, AUDIBLE_ANMR_DB};
use crate::perceptual::{tiled, Playback};
use crate::spectrum::fft;
use crate::system::System;
use crate::tone_window::ToneWindow;
use crate::AUDIBLE_MAX_HZ;

/// One step: a tone's output spectrum, split by frequency.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AliasPoint {
    /// The tone's frequency.
    pub hz: f64,
    /// The fundamental's amplitude in the output, dB (a full-scale sine
    /// is 0 dB).
    pub fundamental_db: f64,
    /// The harmonics `2·f₀, 3·f₀, …` below Nyquist, energy summed, dB
    /// re the fundamental.
    pub harmonics_db: f64,
    /// Every other bin above DC (aliases, and any noise), energy summed,
    /// dB re the fundamental. The aliasing figure.
    pub inharmonic_db: f64,
    /// The inharmonic bins at or below 20 kHz only.
    pub audible_db: f64,
    /// The strongest single inharmonic bin, dB re the fundamental.
    pub worst_db: f64,
    /// Where it is.
    pub worst_hz: f64,
    /// Sato & Smith's aliasing-to-signal ratio, linear: the inharmonic
    /// energy over the fundamental's and harmonics' together, `E_A /
    /// E_H` with `E_A = E_Y − E_H` over the bins above DC. (The paper
    /// sums `E_Y` from bin 0; a DC offset is not a fold, and a network's
    /// biases make one, so it is left out here.)
    pub asr: f64,
}

/// Split one capture (`n` samples, a power of two, a tone at the odd
/// bin `carrier_bin`) into fundamental, harmonics and inharmonic energy.
pub fn classify(capture: &[f64], carrier_bin: usize, sample_rate: f64) -> AliasPoint {
    let n = capture.len();
    let m = carrier_bin;
    assert!(m > 0 && m < n / 2, "carrier bin {m} outside (0, {})", n / 2);
    let mut re = capture.to_vec();
    let mut im = vec![0.0f64; n];
    fft(&mut re, &mut im);
    let bin_hz = sample_rate / n as f64;
    let power = |b: usize| re[b] * re[b] + im[b] * im[b];
    let fundamental = power(m).max(1e-300);
    let (mut harmonics, mut inharmonic, mut audible) = (0.0f64, 0.0f64, 0.0f64);
    let (mut worst, mut worst_bin) = (0.0f64, 0usize);
    for b in 1..=n / 2 {
        if b == m {
            continue;
        }
        let p = power(b);
        if b % m == 0 {
            harmonics += p;
        } else {
            inharmonic += p;
            if b as f64 * bin_hz <= AUDIBLE_MAX_HZ {
                audible += p;
            }
            if p > worst {
                worst = p;
                worst_bin = b;
            }
        }
    }
    let db = |p: f64| 10.0 * (p.max(1e-300) / fundamental).log10();
    AliasPoint {
        hz: m as f64 * bin_hz,
        fundamental_db: 20.0 * (2.0 * fundamental.sqrt() / n as f64).log10(),
        harmonics_db: db(harmonics),
        inharmonic_db: db(inharmonic),
        audible_db: db(audible),
        worst_db: db(worst),
        worst_hz: worst_bin as f64 * bin_hz,
        asr: inharmonic / (fundamental + harmonics),
    }
}

/// Kahles et al.'s ANMR of one capture (`n` samples, a power of two, a
/// tone at the odd bin `carrier_bin`), in dB: the inharmonic bins at or
/// below 20 kHz under the mask the harmonics set, both A-weighted, the
/// capture's peak (about its mean) at full scale (92 dB SPL). The ear model needs a few
/// frames, so the (periodic) capture is repeated to half a second.
pub fn anmr(capture: &[f64], carrier_bin: usize, sample_rate: f64) -> f64 {
    let n = capture.len();
    let m = carrier_bin;
    assert!(m > 0 && m < n / 2, "carrier bin {m} outside (0, {})", n / 2);
    let mut re = capture.to_vec();
    let mut im = vec![0.0f64; n];
    fft(&mut re, &mut im);
    let bin_hz = sample_rate / n as f64;
    // Split the one-sided spectrum and rebuild each part as a signal.
    let part = |keep: &dyn Fn(usize) -> bool| -> Vec<f64> {
        let (mut r, mut i) = (vec![0.0; n], vec![0.0; n]);
        for b in 1..n / 2 {
            if keep(b) {
                r[b] = re[b];
                i[b] = -im[b]; // conjugate: the forward FFT computes the inverse
                r[n - b] = re[b];
                i[n - b] = im[b];
            }
        }
        fft(&mut r, &mut i);
        r.iter().map(|v| v / n as f64).collect()
    };
    let masker = part(&|b| b % m == 0);
    let noise = part(&|b| b % m != 0 && b as f64 * bin_hz <= AUDIBLE_MAX_HZ);
    let mean = capture.iter().sum::<f64>() / n as f64;
    let peak = capture.iter().fold(0.0f64, |p, v| p.max((v - mean).abs())).max(1e-30);
    let len = (sample_rate / 2.0) as usize;
    let (masker, noise) = (tiled(&masker, len), tiled(&noise, len));
    Nmr::anmr(Playback::full_scale(peak), &masker, &noise, sample_rate).total_db
}

/// One step's ANMR.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AnmrPoint {
    pub hz: f64,
    pub anmr_db: f64,
}

/// A stepped ANMR measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct AnmrSweep {
    pub points: Vec<AnmrPoint>,
}

impl AnmrSweep {
    /// The step with the highest ANMR.
    pub fn worst(&self) -> AnmrPoint {
        *self
            .points
            .iter()
            .max_by(|a, b| a.anmr_db.total_cmp(&b.anmr_db))
            .expect("a measurement has at least one step")
    }

    /// Every step at or under `threshold_db` (−10 dB: inaudible).
    pub fn inaudible(&self, threshold_db: f64) -> bool {
        self.points.iter().all(|p| p.anmr_db <= threshold_db)
    }

    /// The steps above the audibility threshold.
    pub fn audible(&self) -> Vec<AnmrPoint> {
        self.points.iter().copied().filter(|p| p.anmr_db > AUDIBLE_ANMR_DB).collect()
    }
}

/// Tones stepped from `low_hz` to `high_hz`, log-spaced, each snapped to
/// an odd bin of `window`, each played into a fresh system for
/// `warmup_windows` and captured for one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SteppedTone {
    pub sample_rate: f64,
    /// Samples per capture, a power of two.
    pub window: usize,
    /// The input tone's peak amplitude (a stage's drive is its own).
    pub amplitude: f64,
    pub low_hz: f64,
    pub high_hz: f64,
    pub steps: usize,
    /// Whole windows played before the capture: past every settling.
    pub warmup_windows: usize,
}

impl Default for SteppedTone {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            window: 8192,
            amplitude: 0.5,
            low_hz: 100.0,
            high_hz: 10_000.0,
            steps: 8,
            warmup_windows: 2,
        }
    }
}

impl SteppedTone {
    /// The piano's range, 27.5 Hz (A0) to 4186 Hz (C8), in `steps`
    /// log-spaced steps (88: every semitone; the window resolves fewer
    /// in the lowest octave, where neighbouring keys share an odd bin).
    pub fn piano(sample_rate: f64, steps: usize) -> Self {
        Self {
            sample_rate,
            low_hz: 27.5,
            high_hz: 4_186.0,
            steps,
            ..Self::default()
        }
    }

    /// Each step's tone through a fresh system from `make`: its ANMR.
    pub fn anmr_through<S: System>(&self, mut make: impl FnMut() -> S) -> AnmrSweep {
        let points = self
            .bins()
            .into_iter()
            .map(|bin| {
                let tone = ToneWindow {
                    bin,
                    window: self.window,
                    sample_rate: self.sample_rate,
                    amplitude: self.amplitude,
                    warmup_windows: self.warmup_windows,
                };
                AnmrPoint {
                    hz: bin as f64 * self.sample_rate / self.window as f64,
                    anmr_db: anmr(&tone.capture(make()), bin, self.sample_rate),
                }
            })
            .collect();
        AnmrSweep { points }
    }

    /// A generator stepped across its range: each step's ANMR.
    pub fn anmr_generated<G: System>(&self, mut make: impl FnMut(f64) -> G) -> AnmrSweep {
        let points = self
            .bins()
            .into_iter()
            .map(|bin| {
                let hz = bin as f64 * self.sample_rate / self.window as f64;
                let mut g = make(hz);
                let silence = vec![0.0f32; self.window];
                let mut out = vec![0.0f32; self.window];
                for _ in 0..=self.warmup_windows {
                    g.process(&silence, &mut out);
                }
                let capture: Vec<f64> = out.into_iter().map(f64::from).collect();
                AnmrPoint {
                    hz,
                    anmr_db: anmr(&capture, bin, self.sample_rate),
                }
            })
            .collect();
        AnmrSweep { points }
    }

    /// The steps' carrier bins: log-spaced, odd, distinct, ascending.
    pub fn bins(&self) -> Vec<usize> {
        assert!(
            self.window.is_power_of_two(),
            "window must be a power of two"
        );
        let to_bin = |hz: f64| {
            let b = (hz * self.window as f64 / self.sample_rate).round() as usize;
            (b | 1).min(self.window / 2 - 1)
        };
        let mut out: Vec<usize> = (0..self.steps.max(1))
            .map(|i| {
                let t = if self.steps > 1 {
                    i as f64 / (self.steps - 1) as f64
                } else {
                    0.0
                };
                to_bin(self.low_hz * (self.high_hz / self.low_hz).powf(t))
            })
            .collect();
        out.dedup();
        out
    }

    /// Each step's tone through a fresh system from `make`.
    pub fn through<S: System>(&self, mut make: impl FnMut() -> S) -> Aliasing {
        let points = self
            .bins()
            .into_iter()
            .map(|bin| {
                let tone = ToneWindow {
                    bin,
                    window: self.window,
                    sample_rate: self.sample_rate,
                    amplitude: self.amplitude,
                    warmup_windows: self.warmup_windows,
                };
                classify(&tone.capture(make()), bin, self.sample_rate)
            })
            .collect();
        Aliasing { points }
    }

    /// A generator stepped across its range: `make(hz)` returns one
    /// running at `hz` (its input is ignored), prepared at
    /// [`sample_rate`](Self::sample_rate).
    pub fn generated<G: System>(&self, mut make: impl FnMut(f64) -> G) -> Aliasing {
        let points = self
            .bins()
            .into_iter()
            .map(|bin| {
                let hz = bin as f64 * self.sample_rate / self.window as f64;
                let mut g = make(hz);
                let silence = vec![0.0f32; self.window];
                let mut out = vec![0.0f32; self.window];
                for _ in 0..=self.warmup_windows {
                    g.process(&silence, &mut out);
                }
                let capture: Vec<f64> = out.into_iter().map(f64::from).collect();
                classify(&capture, bin, self.sample_rate)
            })
            .collect();
        Aliasing { points }
    }
}

/// A stepped measurement: one [`AliasPoint`] per step.
#[derive(Clone, Debug, PartialEq)]
pub struct Aliasing {
    pub points: Vec<AliasPoint>,
}

impl Aliasing {
    /// The step with the most inharmonic energy.
    pub fn worst(&self) -> AliasPoint {
        self.worst_by(|p| p.inharmonic_db)
    }

    /// The step with the most inharmonic energy at or below 20 kHz.
    pub fn worst_audible(&self) -> AliasPoint {
        self.worst_by(|p| p.audible_db)
    }

    /// The step with the highest aliasing-to-signal ratio.
    pub fn worst_asr(&self) -> AliasPoint {
        self.worst_by(|p| p.asr)
    }

    fn worst_by(&self, key: impl Fn(&AliasPoint) -> f64) -> AliasPoint {
        *self
            .points
            .iter()
            .max_by(|a, b| key(a).total_cmp(&key(b)))
            .expect("a measurement has at least one step")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square law makes exactly one harmonic; at a high enough tone it
    /// folds, and the fold is counted as inharmonic at its frequency.
    #[test]
    fn harmonics_and_folds_are_told_apart_by_frequency() {
        let fs = 48_000.0;
        let n = 8192;
        // 1 kHz at 0.5: x² makes 2 kHz at 0.125, 12 dB under, and DC.
        let low = SteppedTone {
            low_hz: 1_000.0,
            high_hz: 1_000.0,
            steps: 1,
            ..SteppedTone::default()
        };
        let p = low.through(|| |x: f32| x + x * x).worst();
        assert!(
            (p.harmonics_db - 20.0 * 0.25f64.log10()).abs() < 0.01,
            "{p:?}"
        );
        assert!(p.inharmonic_db < -120.0, "{p:?}");
        // 15 kHz (odd bin 2561): 2·f₀ = 30 kHz folds to 18 kHz.
        let high = SteppedTone {
            low_hz: 15_000.0,
            high_hz: 15_000.0,
            steps: 1,
            ..SteppedTone::default()
        };
        let p = high.through(|| |x: f32| x + x * x).worst();
        assert!(p.harmonics_db < -120.0, "{p:?}");
        assert!(
            (p.worst_hz - (fs - 2.0 * p.hz)).abs() < fs / n as f64,
            "{p:?}"
        );
        assert!((p.inharmonic_db - p.worst_db).abs() < 1e-6, "{p:?}");
        assert!((p.audible_db - p.inharmonic_db).abs() < 1e-6, "{p:?}");
    }

    /// The aliasing-to-signal ratio divides by every harmonic, the dB
    /// figure by the fundamental alone: for x + x² at 15 kHz (no
    /// harmonic below Nyquist, one fold) they agree; at 1 kHz with a
    /// fold-free harmonic the ratio is zero.
    #[test]
    fn asr_divides_by_the_whole_harmonic_energy() {
        let tone = |hz: f64| SteppedTone {
            low_hz: hz,
            high_hz: hz,
            steps: 1,
            ..SteppedTone::default()
        };
        let high = tone(15_000.0).through(|| |x: f32| x + x * x).worst();
        assert!(
            (10.0 * high.asr.log10() - high.inharmonic_db).abs() < 1e-6,
            "{high:?}"
        );
        let low = tone(1_000.0).through(|| |x: f32| x + x * x).worst();
        assert!(low.asr < 1e-12, "{low:?}");
        // A hard clip: harmonics carry energy, so the ratio sits below the
        // dB-re-fundamental figure.
        let clip = tone(5_000.0)
            .through(|| |x: f32| (4.0 * x).clamp(-1.0, 1.0))
            .worst();
        assert!(10.0 * clip.asr.log10() < clip.inharmonic_db, "{clip:?}");
    }

    /// Steps are odd bins, log-spaced across the range.
    #[test]
    fn steps_are_odd_and_ascending() {
        let bins = SteppedTone::default().bins();
        assert_eq!(bins.len(), 8);
        assert!(bins.iter().all(|b| b % 2 == 1));
        assert!(bins.windows(2).all(|w| w[0] < w[1]));
    }

    /// Kahles et al.'s figure 10, in shape: a hard clip (threshold 0.1)
    /// at 44.1 kHz with no oversampling aliases audibly (ANMR above
    /// −10 dB) over the upper piano range; a gain makes no aliases and
    /// reads at the floor; a clean tone at the floor too.
    #[test]
    fn anmr_flags_a_hard_clip_and_passes_a_gain() {
        let sweep = SteppedTone {
            low_hz: 500.0,
            high_hz: 4_186.0,
            steps: 4,
            ..SteppedTone::piano(44_100.0, 4)
        };
        let clip = sweep.anmr_through(|| |x: f32| x.clamp(-0.1, 0.1));
        assert!(clip.worst().anmr_db > AUDIBLE_ANMR_DB, "{clip:?}");
        assert!(clip.points.last().unwrap().anmr_db > 0.0, "{clip:?}");
        let gain = sweep.anmr_through(|| |x: f32| 0.5 * x);
        assert!(gain.worst().anmr_db < -60.0, "{gain:?}");
    }

    /// ANMR rises with the aliased energy dB for dB: the same fold at
    /// half the level reads 6 dB lower.
    #[test]
    fn anmr_follows_the_alias_level() {
        let n = 8192;
        let fs = 48_000.0;
        let m = 1001; // ≈ 5.86 kHz
        let tone = |b: usize, a: f64| (0..n).map(move |i| a * (std::f64::consts::TAU * (b * i) as f64 / n as f64).sin());
        // A fundamental, a harmonic, and an alias between them.
        let make = |alias: f64| -> Vec<f64> {
            tone(m, 0.5).zip(tone(2 * m, 0.1)).zip(tone(1500, alias)).map(|((a, b), c)| a + b + c).collect()
        };
        let (hi, lo) = (anmr(&make(1e-3), m, fs), anmr(&make(5e-4), m, fs));
        assert!((hi - lo - 6.02).abs() < 0.05, "{hi} {lo}");
    }

    /// A naive saw from a phase accumulator aliases; the measurement
    /// finds it at the predicted frequency.
    #[test]
    fn a_naive_saw_aliases_where_predicted() {
        let sweep = SteppedTone {
            low_hz: 3_000.0,
            high_hz: 3_000.0,
            steps: 1,
            ..SteppedTone::default()
        };
        let r = sweep.generated(|hz| {
            let inc = hz / 48_000.0;
            let mut t = 0.0f64;
            move |_: f32| {
                let y = 2.0 * t - 1.0;
                t = (t + inc).fract();
                y as f32
            }
        });
        let p = r.worst();
        assert!(p.inharmonic_db > -30.0, "{p:?}");
        // The loudest fold is the 9th partial (27 kHz → 21 kHz) or the
        // 8th's neighbour: whichever, it is not a harmonic.
        let k = p.worst_hz / p.hz;
        assert!((k - k.round()).abs() > 0.01, "{p:?}");
    }
}
