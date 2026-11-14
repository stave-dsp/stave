//! `SubOctave` — constructed subharmonics. A period-tracking divider in the classic octave-fuzz
//! lineage: band-limited detector → comparator with envelope-scaled
//! hysteresis → ÷2 flip-flop → envelope-tracked, band-limited output.
//!
//! Two uses, one type:
//! * SIGNAL — mix the output under the program (octave-down growl).
//! * DRIVER — `Driver::Sub` gain-modulates a stage at f0/2, which
//!   puts energy at 1.5·f0, 2.5·f0, … — literally between the integer
//!   harmonics, something no memoryless curve can do.
//!
//! Known limit (by design, measured in conformance): monophonic-solid.
//! On polyphonic/ambiguous input the envelope-scaled hysteresis keeps
//! the divider bounded and it degrades toward gated quiet rather than
//! full-scale glitch. Real polyphonic tracking is a separate future
//! investment.

use crate::kernel::control::Source;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::units::Hz;
use crate::units::seconds;

#[derive(Clone)]
pub struct SubOctave {
    /// Detector band-limit: highpass clears DC/rumble so the comparator
    /// sees zero-mean material; lowpass keeps upper harmonics from
    /// double-triggering the crossing detector (the classic divider
    /// failure).
    det_hp: Biquad,
    det_lp: Biquad,
    /// Envelope of the detector (fast attack) — scales the comparator
    /// hysteresis AND the output level, and gates silence.
    env: f32,
    env_atk: f32,
    env_rel: f32,
    /// Comparator + flip-flop state.
    comp_high: bool,
    ff: f32,
    /// Output shaping: one-pole smoothing turns the flip-flop step into
    /// an exponential edge (−6 dB/oct past ~400 Hz — keeps discrete-time
    /// step aliasing below the audible-alias limits), then a gentle
    /// lowpass voices it.
    smooth: f32,
    smooth_coeff: f32,
    out_lp: Biquad,
    /// Hysteresis as a fraction of the envelope, with an absolute floor
    /// so noise floors can't flap the divider.
    hyst_frac: f32,
    gate_floor: f32,
}

impl Default for SubOctave {
    fn default() -> Self {
        Self::new()
    }
}

impl SubOctave {
    pub fn new() -> Self {
        let mut s = Self {
            det_hp: Biquad::new(FilterSpec::Highpass { frequency: Hz::new(35.0), q: 0.707 }),
            det_lp: Biquad::new(FilterSpec::Lowpass {
                frequency: Hz::new(900.0),
                q: 0.707,
            }),
            env: 0.0,
            env_atk: 0.0,
            env_rel: 0.0,
            comp_high: false,
            ff: 1.0,
            smooth: 0.0,
            smooth_coeff: 0.0,
            out_lp: Biquad::new(FilterSpec::Lowpass {
                frequency: Hz::new(1_500.0),
                q: 0.707,
            }),
            hyst_frac: 0.35,
            gate_floor: 1.5e-3,
        };
        s.derive(48_000.0);
        s
    }

    fn derive(&mut self, sample_rate: f32) {
        // Envelope: 1 ms attack / 60 ms release.
        self.env_atk = tau_coeff(seconds(0.001), sample_rate);
        self.env_rel = tau_coeff(seconds(0.060), sample_rate);
        // Edge smoothing ≈ 400 Hz one-pole.
        self.smooth_coeff = 1.0 - (-2.0 * core::f32::consts::PI * 400.0 / sample_rate).exp();
    }
}

impl Stage for SubOctave {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.det_hp.prepare(sample_rate);
        self.det_lp.prepare(sample_rate);
        self.out_lp.prepare(sample_rate);
        self.derive(sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.det_hp.reset();
        self.det_lp.reset();
        self.out_lp.reset();
        self.env = 0.0;
        self.comp_high = false;
        self.ff = 1.0;
        self.smooth = 0.0;
    }

    /// Advance one sample. `x` is the SIGNED program signal (not a
    /// rectified level) — the divider needs the waveform's crossings.
    /// Returns the band-limited sub-octave, level-tracked to the input.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let d = self.det_lp.tick(self.det_hp.tick(x));
        let t = d.abs();
        let c = if t > self.env {
            self.env_atk
        } else {
            self.env_rel
        };
        self.env = crate::kernel::denormal::flush(self.env + (t - self.env) * c);
        // Hysteresis rides the envelope; the floor gates noise.
        let th = (self.hyst_frac * self.env).max(self.gate_floor);
        if !self.comp_high && d > th {
            self.comp_high = true;
            self.ff = -self.ff; // ÷2: one toggle per input period
        } else if self.comp_high && d < -th {
            self.comp_high = false;
        }
        // Gate: below the floor the divider contributes nothing.
        let gated = if self.env > self.gate_floor {
            self.ff * self.env
        } else {
            0.0
        };
        self.smooth += (gated - self.smooth) * self.smooth_coeff;
        self.out_lp.tick(self.smooth)
    }

    /// The envelope's release, then the output filter's ring.
    fn tail(&self) -> Tail {
        Tail::from_pole(f64::from(1.0 - self.env_rel)).then(self.out_lp.tail())
    }
}

impl Source for SubOctave {
    /// Last output — bipolar, for driver use.
    fn state(&self) -> f32 {
        self.smooth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
        let w = 2.0 * core::f64::consts::PI * f / fs;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        ((s1 - s2 * w.cos()).powi(2) + (s2 * w.sin()).powi(2)).sqrt() / (buf.len() as f64 * 0.5)
    }

    fn run_tone(sub: &mut SubOctave, f0: f64, amp: f64, n: usize, fs: f64) -> Vec<f32> {
        (0..n)
            .map(|i| {
                sub.tick((amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin()) as f32)
            })
            .collect()
    }

    #[test]
    fn locks_at_half_frequency_and_tracks_level() {
        let fs = 48_000.0;
        for f0 in [93.75f64, 187.5, 375.0, 750.0] {
            let mut sub = SubOctave::new();
            sub.prepare(fs as f32);
            let _ = run_tone(&mut sub, f0, 0.5, 24_000, fs);
            let out = run_tone(&mut sub, f0, 0.5, 32_768, fs);
            let at_half = goertzel(&out, f0 / 2.0, fs);
            let at_f0 = goertzel(&out, f0, fs);
            assert!(
                at_half > 4.0 * at_f0.max(1e-9),
                "{f0} Hz: sub should dominate: f0/2 {at_half:.4} vs f0 {at_f0:.4}"
            );
        }
        // Level tracking: 14 dB more input → about 14 dB more sub.
        let level_of = |amp: f64| -> f64 {
            let mut sub = SubOctave::new();
            sub.prepare(48_000.0);
            let _ = run_tone(&mut sub, 187.5, amp, 24_000, 48_000.0);
            let out = run_tone(&mut sub, 187.5, amp, 32_768, 48_000.0);
            20.0 * goertzel(&out, 93.75, 48_000.0).log10()
        };
        let ratio = level_of(0.5) - level_of(0.1);
        assert!(
            (ratio - 13.98).abs() < 1.5,
            "level tracking off: {ratio:.2} dB for 14 dB in"
        );
    }

    #[test]
    fn gates_on_silence_and_retriggers_clean() {
        let fs = 48_000.0;
        let mut sub = SubOctave::new();
        sub.prepare(fs as f32);
        // Note → silence → note.
        let _ = run_tone(&mut sub, 187.5, 0.5, 24_000, fs);
        // The 60 ms release needs ~0.4 s to fall below the gate floor —
        // measure after that.
        let quiet: Vec<f32> = (0..36_000).map(|_| sub.tick(0.0)).collect();
        let tail_rms = {
            let t = &quiet[24_000..];
            (t.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / t.len() as f64).sqrt()
        };
        assert!(
            tail_rms < 1e-4,
            "silence should gate the divider: {tail_rms:.6}"
        );
        let _ = run_tone(&mut sub, 187.5, 0.5, 12_000, fs);
        let out = run_tone(&mut sub, 187.5, 0.5, 32_768, fs);
        let at_half = goertzel(&out, 93.75, fs);
        assert!(at_half > 0.1, "retrigger should re-lock: {at_half:.4}");
        // Sub-floor input stays silent (hysteresis floor).
        let mut sub2 = SubOctave::new();
        sub2.prepare(fs as f32);
        let out2 = run_tone(&mut sub2, 187.5, 0.0008, 48_000, fs);
        let rms =
            (out2.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / out2.len() as f64).sqrt();
        assert!(
            rms < 1e-4,
            "sub-floor input should not wake the divider: {rms:.6}"
        );
    }

    #[test]
    fn polyphonic_input_degrades_bounded_not_glitchy() {
        let fs = 48_000.0;
        let mut sub = SubOctave::new();
        sub.prepare(fs as f32);
        // An ambiguous two-tone (minor seventh-ish) — no stable period.
        let out: Vec<f32> = (0..96_000)
            .map(|i| {
                let t = i as f64 / fs;
                let x = 0.35 * (2.0 * core::f64::consts::PI * 187.5 * t).sin()
                    + 0.35 * (2.0 * core::f64::consts::PI * 333.0 * t).sin();
                sub.tick(x as f32)
            })
            .collect();
        let peak = out.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(
            peak.is_finite() && peak < 1.0,
            "divider must stay bounded: {peak}"
        );
    }
}
