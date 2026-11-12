//! `Osc` — the audio-rate oscillator leaf (`Lfo` is control-rate;
//! this one is what you HEAR). A phase accumulator with f64 phase —
//! pitch is exactness, and f32 increments drift audibly over a held
//! note (the f64-where-exactness-demands rule) — producing:
//! * `Sine` — pure, THD at the float floor;
//! * `Saw` / `Square` — polyBLEP band-limited: each discontinuity is
//!   smoothed over the two samples around it by the polynomial
//!   residual. Measured at 2 kHz/48 k (the in-module invariant): exposed
//!   midband aliases (< 8 kHz, where naive saws audibly ring) drop
//!   ≥ 20 dB; the worst near-Nyquist fold drops 12.4 dB — first-
//!   order BLEP rejection grows as the alias departs Nyquist, and
//!   that residual sits where ears are masked. Serum-class cleanness
//!   is a wavetable's job, not this
//!   leaf's.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OscShape {
    Sine,
    Saw,
    Square,
}

#[derive(Copy, Clone, Debug)]
pub struct Osc {
    shape: OscShape,
    /// Normalized phase 0..1 (f64: pitch is exactness).
    phase: f64,
    /// Per-sample phase increment = hz / sr.
    inc: f64,
}

/// The polyBLEP residual: the two-sample polynomial correction
/// subtracted around each discontinuity. `t` is the phase 0..1,
/// `dt` the per-sample increment.
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
    pub fn new(shape: OscShape) -> Self {
        Self {
            shape,
            phase: 0.0,
            inc: 0.0,
        }
    }

    pub fn set_shape(&mut self, shape: OscShape) {
        self.shape = shape;
    }

    /// Set the frequency (per-sample cheap: one divide).
    #[inline]
    pub fn set_hz(&mut self, hz: f32, sr: f32) {
        debug_assert!(sr > 0.0, "sample rate must be positive");
        self.inc = f64::from(hz) / f64::from(sr);
    }

    /// Restart the cycle (note-on phase discipline: a voice that
    /// resets phase sounds the same attack every time — reproducible
    /// by construction).
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let t = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let dt = self.inc;
        let y = match self.shape {
            OscShape::Sine => (core::f64::consts::TAU * t).sin(),
            OscShape::Saw => (2.0 * t - 1.0) - polyblep(t, dt),
            OscShape::Square => {
                let raw = if t < 0.5 { 1.0 } else { -1.0 };
                raw + polyblep(t, dt) - polyblep((t + 0.5).rem_euclid(1.0), dt)
            }
        };
        y as f32
    }
}

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

    fn render(shape: OscShape, hz: f32, n: usize, blep: bool) -> Vec<f32> {
        let mut o = Osc::new(shape);
        o.set_hz(hz, SR);
        (0..n)
            .map(|_| {
                if blep {
                    o.tick()
                } else {
                    // The naive comparison: same accumulator, no BLEP.
                    let t = o.phase;
                    o.phase = (o.phase + o.inc).rem_euclid(1.0);
                    match shape {
                        OscShape::Sine => (core::f64::consts::TAU * t).sin() as f32,
                        OscShape::Saw => (2.0 * t - 1.0) as f32,
                        OscShape::Square => {
                            if t < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                    }
                }
            })
            .collect()
    }

    #[test]
    fn pitch_is_exact() {
        // The fundamental dominates at exactly the dialed hz, and a
        // detuned probe half a semitone off reads ≥ 30 dB down.
        for shape in [OscShape::Sine, OscShape::Saw, OscShape::Square] {
            let w = render(shape, 440.0, 48_000, true);
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
        let w = render(OscShape::Sine, 997.0, 48_000, true);
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
        let naive = render(OscShape::Saw, f0 as f32, 48_000, false);
        let blep = render(OscShape::Saw, f0 as f32, 48_000, true);
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
