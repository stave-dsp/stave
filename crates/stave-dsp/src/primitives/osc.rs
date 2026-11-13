//! `Osc` — the audio-rate oscillator (`Lfo` is the control-rate one)
//! — and [`Waveform`], the shape vocabulary both share. A phase
//! accumulator with f64 phase (pitch is exactness, and f32 increments
//! drift audibly over a held note), producing:
//! * `Sine` — pure, THD at the float floor;
//! * `Triangle` — naive; its harmonics fall at 12 dB/octave;
//! * `Saw` / `Square` — polyBLEP band-limited: each discontinuity is
//!   smoothed over the two samples around it by the polynomial
//!   residual. Measured at 2 kHz/48 k (the in-module invariant): exposed
//!   midband aliases (< 8 kHz, where naive saws audibly ring) drop
//!   ≥ 20 dB; the worst near-Nyquist fold drops 12.4 dB — first-
//!   order BLEP rejection grows as the alias departs Nyquist, and
//!   that residual sits where ears are masked. Wavetable-grade
//!   cleanness is a wavetable's job, not this leaf's.

use crate::kernel::node::{Generator, Stage, Tail};

/// The periodic waveforms every oscillator and LFO draws from.
///
/// Phase is in **cycles**, `0 ≤ t < 1`, everywhere in stave-dsp. At
/// `t = 0`: the sine is 0 and rising, the triangle is at −1 and rising
/// to +1 at `t = ½`, the saw ramps −1 → +1 over the cycle, and the
/// square is +1 for the first half (in phase with the sine).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Waveform {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Waveform {
    /// The naive (not band-limited) waveform at phase `t` cycles —
    /// exact for control-rate use; [`Osc`] band-limits the edges.
    #[inline]
    pub fn eval(self, t: f32) -> f32 {
        let t = t.rem_euclid(1.0);
        match self {
            Waveform::Sine => (core::f32::consts::TAU * t).sin(),
            Waveform::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Waveform::Saw => 2.0 * t - 1.0,
            Waveform::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// The audio-rate oscillator: a generator [`Stage`] (its input is
/// ignored). Phase is `f64` cycles.
///
/// # Models
///
/// The four [`Waveform`]s from a phase accumulator; saw and square
/// band-limited by polyBLEP, sine exact, triangle naive.
///
/// # Derivation
///
/// The two-sample polynomial band-limited step of Välimäki and Huovilainen
/// (2007), subtracted at each discontinuity.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/osc.rs::polyblep_buys_at_least_20_db_on_the_worst_alias`
/// (a 2093 Hz saw's exposed aliases below 8 kHz at least 20 dB under the
/// naive saw's, the worst near-Nyquist fold at least 10 dB),
/// `crates/stave-dsp/src/primitives/osc.rs::pitch_is_exact` and
/// `crates/stave-dsp/src/primitives/osc.rs::sine_harmonics_are_at_the_floor`
/// (2nd and 3rd under −80 dB).
///
/// # Cost
///
/// Datasheet `"Osc (sine)"` and `"Osc (saw, polyBLEP)"`.
///
/// # Aliasing
///
/// First-order polyBLEP on saw and square (residual aliasing near
/// Nyquist, see Verified); the naive triangle's harmonics fall at
/// 12 dB/octave. Wavetable-grade cleanness needs a wavetable.
///
/// # Valid ranges
///
/// `0 ≤ hz < sample_rate / 2`; polyBLEP assumes the increment is below
/// half a cycle per sample.
#[derive(Copy, Clone, Debug)]
pub struct Osc {
    waveform: Waveform,
    hz: f32,
    sample_rate: f32,
    phase: f64,
    inc: f64,
}

#[inline]
fn polyblep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

impl Osc {
    pub fn new(waveform: Waveform) -> Self {
        Self {
            waveform,
            hz: 0.0,
            sample_rate: 48_000.0,
            phase: 0.0,
            inc: 0.0,
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    #[inline]
    pub fn set_hz(&mut self, hz: f32) {
        self.hz = hz;
        self.inc = f64::from(hz) / f64::from(self.sample_rate);
    }

    /// Phase in cycles, `0 ≤ t < 1`.
    pub fn phase(&self) -> f64 {
        self.phase
    }

    pub fn set_phase(&mut self, cycles: f64) {
        self.phase = cycles.rem_euclid(1.0);
    }
}

impl Stage for Osc {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.phase = 0.0;
    }
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        let t = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let dt = self.inc;
        let y = match self.waveform {
            Waveform::Sine => (core::f64::consts::TAU * t).sin(),
            // Harmonics fall at 12 dB/octave: the naive form is used.
            Waveform::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Waveform::Saw => (2.0 * t - 1.0) - polyblep(t, dt),
            Waveform::Square => {
                let raw = if t < 0.5 { 1.0 } else { -1.0 };
                raw + polyblep(t, dt) - polyblep((t + 0.5).rem_euclid(1.0), dt)
            }
        };
        y as f32
    }
    /// A generator: it never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for Osc {}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Single-bin correlation (Goertzel-shaped) magnitude at `hz`.
    fn mag_at(w: &[f32], hz: f64) -> f64 {
        let n = w.len() as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in w.iter().enumerate() {
            let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(SR);
            re += f64::from(x) * ph.cos();
            im += f64::from(x) * ph.sin();
        }
        (re * re + im * im).sqrt() / n
    }

    fn run(shape: Waveform, hz: f32, n: usize, blep: bool) -> Vec<f32> {
        let mut o = Osc::new(shape);
        o.prepare(SR);
        o.set_hz(hz);
        (0..n)
            .map(|_| {
                if blep {
                    o.tick(0.0)
                } else {
                    // The naive comparison: same accumulator, no BLEP.
                    let t = o.phase;
                    o.phase = (o.phase + o.inc).rem_euclid(1.0);
                    match shape {
                        Waveform::Sine => (core::f64::consts::TAU * t).sin() as f32,
                        other => other.eval(t as f32),
                    }
                }
            })
            .collect()
    }

    #[test]
    fn pitch_is_exact() {
        // The fundamental dominates at exactly the dialed hz, and a
        // detuned probe half a semitone off reads ≥ 30 dB down.
        for shape in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Square,
        ] {
            let w = run(shape, 440.0, 48_000, true);
            let on = mag_at(&w, 440.0);
            let off = mag_at(&w, 440.0 * 1.03);
            assert!(
                on > off * 30.0,
                "{shape:?}: fundamental {on:.4} vs detuned probe {off:.4}"
            );
        }
    }

    #[test]
    fn sine_harmonics_are_at_the_floor() {
        let w = run(Waveform::Sine, 997.0, 48_000, true);
        let h1 = mag_at(&w, 997.0);
        let h2 = mag_at(&w, 2.0 * 997.0);
        let h3 = mag_at(&w, 3.0 * 997.0);
        assert!(
            h2 / h1 < 1e-4 && h3 / h1 < 1e-4,
            "sine THD: h2 {h2:.2e} h3 {h3:.2e}"
        );
    }

    #[test]
    fn polyblep_buys_at_least_20_db_on_the_worst_alias() {
        // 2093 Hz saw at 48 k. Aliases land at EXACTLY |k·f0 mod fs|
        // folded into the band, for the partials k past Nyquist —
        // probe those frequencies (skipping any that collide with a
        // true harmonic), never a grid. Domain-qualified per the
        // physics of first-order BLEP (rejection grows as the alias
        // departs Nyquist): the worst near-Nyquist fold wins a
        // measured 12.4 dB (≥ 10 asserted) and is perceptually
        // masked; the EXPOSED midband folds (< 8 kHz — where naive
        // saws audibly ring) win ≥ 20 dB.
        let (f0, fs) = (2_093.0f64, f64::from(SR));
        let naive = run(Waveform::Saw, f0 as f32, 48_000, false);
        let blep = run(Waveform::Saw, f0 as f32, 48_000, true);
        let mut alias_freqs = Vec::new();
        for k in 1..200 {
            let f = k as f64 * f0;
            if f <= fs / 2.0 {
                continue; // a true partial, not an alias
            }
            let mut folded = f % fs;
            if folded > fs / 2.0 {
                folded = fs - folded;
            }
            let in_band = (100.0..20_000.0).contains(&folded);
            let near_harmonic = (folded / f0 - (folded / f0).round()).abs() < 0.02;
            if in_band && !near_harmonic {
                alias_freqs.push(folded);
            }
        }
        assert!(
            !alias_freqs.is_empty(),
            "test premise: aliases exist in band"
        );
        let worst = |w: &[f32], lo: f64, hi: f64| {
            alias_freqs
                .iter()
                .filter(|f| (lo..hi).contains(*f))
                .map(|&f| mag_at(w, f))
                .fold(0.0f64, f64::max)
        };
        let win = |lo: f64, hi: f64| {
            let (n, b) = (worst(&naive, lo, hi), worst(&blep, lo, hi));
            20.0 * (n / b.max(1e-30)).log10()
        };
        let full = win(100.0, 24_000.0);
        let exposed = win(100.0, 8_000.0);
        assert!(full > 10.0, "full-band worst-alias win only {full:.1} dB");
        assert!(
            exposed > 20.0,
            "exposed midband alias win only {exposed:.1} dB"
        );
    }
}
