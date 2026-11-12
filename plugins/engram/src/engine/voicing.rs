//! `Voicing` — the generated room's TONE (on the whole room: reflections
//! and tail) and WIDTH (on the late tail; the reflections have their own).
//!
//! Tone: the decoded room's energy per octave, MINUS what a WHITE room
//! would have there (octaves widen upward: +3 dB each — the late network
//! and the reflections are white-ish already), MINUS what its per-band
//! decay already makes (a band that rings twice as long has twice the
//! energy — room.rs does that part). What is left is the room's colour
//! at the source (a bright plate, a dull chamber), applied as low / high
//! bands around the mid (crossovers 350 Hz, 3 kHz), LOUDNESS-NEUTRAL:
//! the room's level is set elsewhere; colour only redistributes it.
//!
//! Width: the late network's L/R are nearly uncorrelated; the side is
//! scaled so their correlation is the room's, ρ = (1 − w²)/(1 + w²).
//!
//! Both scale with `gen` (0 = untouched).

use stave_dsp::Svf;

use super::space::{Anatomy, OCTAVES};

const CROSSOVER_HZ: (f32, f32) = (350.0, 3_000.0);
const BUTTERWORTH: f32 = 1.0 - core::f32::consts::FRAC_1_SQRT_2;
const MAX_DB: f32 = 9.0;
/// The IR measurement's band widths, Hz (octaves round 125 Hz … 8 kHz;
/// the outer two run to 20 Hz and 20 kHz).
const BANDWIDTH: [f32; OCTAVES] = [157.0, 177.0, 354.0, 707.0, 1414.0, 2828.0, 14343.0];
/// Rough share of a mix's energy below 350 Hz, between, above 3 kHz
/// (what loudness-neutral is weighed by).
const WEIGHT: [f32; 3] = [0.45, 0.42, 0.13];
const GLIDE: f32 = 0.3;

pub(super) struct Voicing {
    low: Svf,
    high: Svf,
    /// Linear gains (low, high) and the side's width; glided per block.
    gain: [f32; 2],
    /// The mid's gain (the loudness-neutral normalisation).
    mid: f32,
    width: f32,
    /// Per-sample ramps toward the block's (glided) targets — a per-block
    /// step is a click.
    gain_step: [f32; 2],
    mid_step: f32,
    width_step: f32,
}

/// The tone left after the decay's share, dB, as (low, high) vs the mid.
pub(super) fn colour(a: &Anatomy) -> (f32, f32) {
    let mean_ln_t: f32 = a.t60.iter().map(|t| t.ln()).sum::<f32>() / OCTAVES as f32;
    let mean_ln_bw: f32 = BANDWIDTH.iter().map(|b| b.ln()).sum::<f32>() / OCTAVES as f32;
    let c: [f32; OCTAVES] = core::array::from_fn(|k| {
        a.tone_db[k] - 10.0 * ((a.t60[k].ln() - mean_ln_t) + (BANDWIDTH[k].ln() - mean_ln_bw)) / core::f32::consts::LN_10
    });
    let low = (c[0] + c[1]) / 2.0;
    let mid = (c[2] + c[3] + c[4]) / 3.0;
    let high = (c[5] + c[6]) / 2.0;
    ((low - mid).clamp(-MAX_DB, MAX_DB), (high - mid).clamp(-MAX_DB, MAX_DB))
}

impl Voicing {
    pub(super) fn new() -> Self {
        Self { low: Svf::new(), high: Svf::new(), gain: [1.0; 2], mid: 1.0, width: 1.0, gain_step: [0.0; 2], mid_step: 0.0, width_step: 0.0 }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.low.prepare(sr);
        self.high.prepare(sr);
        self.low.set(CROSSOVER_HZ.0, BUTTERWORTH);
        self.high.set(CROSSOVER_HZ.1, BUTTERWORTH);
        self.gain_step = [0.0; 2];
        self.mid_step = 0.0;
        self.width_step = 0.0;
    }

    /// Once a block; the gains ramp over its `frames`.
    pub(super) fn aim(&mut self, a: &Anatomy, gen: f32, settle: bool, frames: usize) {
        let (lo, hi) = colour(a);
        let (l, h) = (10f32.powf(gen * lo / 20.0), 10f32.powf(gen * hi / 20.0));
        // Loudness-neutral: the weighted energy of (l, 1, h) is 1.
        let norm = ((WEIGHT[0] * l * l + WEIGHT[1] + WEIGHT[2] * h * h) / (WEIGHT[0] + WEIGHT[1] + WEIGHT[2])).sqrt().recip();
        let rho = a.late_corr.clamp(-0.9, 0.95);
        let w = ((1.0 - rho) / (1.0 + rho)).sqrt();
        let targets = [l * norm, h * norm, norm, 1.0 + gen * (w - 1.0)];
        let per = GLIDE / frames.max(1) as f32;
        let mut cur = [self.gain[0], self.gain[1], self.mid, self.width];
        let mut step = [0.0f32; 4];
        for k in 0..4 {
            if settle {
                cur[k] = targets[k];
            } else {
                step[k] = (targets[k] - cur[k]) * per;
            }
        }
        [self.gain[0], self.gain[1], self.mid, self.width] = cur;
        self.gain_step = [step[0], step[1]];
        self.mid_step = step[2];
        self.width_step = step[3];
    }

    /// The late tail's width.
    #[inline]
    pub(super) fn widen(&mut self, x: [f32; 2]) -> [f32; 2] {
        self.width += self.width_step;
        let m = 0.5 * (x[0] + x[1]);
        let s = 0.5 * (x[0] - x[1]) * self.width;
        [m + s, m - s]
    }

    /// The room's colour (reflections + tail).
    #[inline]
    pub(super) fn colour_tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        self.gain[0] += self.gain_step[0];
        self.gain[1] += self.gain_step[1];
        self.mid += self.mid_step;
        let mut y = [0.0f32; 2];
        for (c, (yc, xc)) in y.iter_mut().zip(x).enumerate() {
            let low = self.low.process_lp(xc, c);
            let high = self.high.process(xc, c).hp;
            *yc = low * self.gain[0] + (xc - low - high) * self.mid + high * self.gain[1];
        }
        y
    }

    #[cfg(test)]
    fn tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        let y = self.colour_tick(x);
        self.widen(y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::space::{at_map, decode};

    /// At gen 0 it passes the signal through (the bands sum back).
    #[test]
    fn untouched_at_zero() {
        let mut v = Voicing::new();
        v.prepare(48_000.0);
        v.aim(&decode(&at_map(0.2, 0.8)), 0.0, true, 256);
        let mut worst = 0.0f32;
        for i in 0..4800 {
            let x = [((i * 7919) % 101) as f32 / 50.0 - 1.0, ((i * 104_729) % 97) as f32 / 48.0 - 1.0];
            let y = v.tick(x);
            worst = worst.max((y[0] - x[0]).abs()).max((y[1] - x[1]).abs());
        }
        assert!(worst < 1e-5, "{worst}");
    }

    /// Width hits the room's late correlation on uncorrelated input.
    #[test]
    fn width_sets_the_correlation() {
        let mut a = decode(&at_map(0.5, 0.5));
        a.late_corr = 0.6;
        let mut v = Voicing::new();
        v.prepare(48_000.0);
        v.aim(&a, 1.0, true, 256);
        let mut rng = stave_dsp::Xorshift32::new(7);
        let (mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64);
        for _ in 0..96_000 {
            let y = v.tick([rng.next_bipolar(), rng.next_bipolar()]);
            ll += f64::from(y[0] * y[0]);
            rr += f64::from(y[1] * y[1]);
            lr += f64::from(y[0] * y[1]);
        }
        let corr = lr / (ll * rr).sqrt();
        assert!((corr - 0.6).abs() < 0.05, "corr {corr}");
    }
}
