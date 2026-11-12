//! `Ensemble` — CHORUS: three drifting copies of the strings, each a
//! short delay swept by its own slow sine (120° apart, mirrored
//! between the ears), blended under the original — the string-machine
//! width. Switching it fades the blend, never steps it.

use core::f32::consts::TAU;

use stave_dsp::DelayLine;

const BASE_MS: f32 = 11.0;
/// Sweep depth at full DRIFT, ms.
const DEPTH_MS: f32 = 3.5;
const RATES_HZ: [f32; 3] = [0.51, 0.73, 0.97];
const FADE_MS: f32 = 30.0;

pub(super) struct Ensemble {
    sr: f32,
    lines: [DelayLine; 2],
    phase: [f32; 3],
    amount: f32,
}

impl Ensemble {
    pub(super) fn new() -> Self {
        Self { sr: 48_000.0, lines: [DelayLine::new(), DelayLine::new()], phase: [0.0, 1.0 / 3.0, 2.0 / 3.0], amount: 0.0 }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let n = ((BASE_MS + DEPTH_MS + 2.0) * 0.001 * sr) as usize + 8;
        for l in &mut self.lines {
            l.prepare(n);
        }
        self.phase = [0.0, 1.0 / 3.0, 2.0 / 3.0];
        self.amount = 0.0;
    }

    pub(super) fn reset_to(&mut self, on: bool) {
        self.amount = if on { 1.0 } else { 0.0 };
    }

    #[inline]
    pub(super) fn tick(&mut self, x: [f32; 2], on: bool, depth: f32) -> [f32; 2] {
        let target = if on { 1.0 } else { 0.0 };
        let fade = 1000.0 / (FADE_MS * self.sr);
        self.amount += (target - self.amount).clamp(-fade, fade);
        for (l, v) in self.lines.iter_mut().zip(x) {
            l.write(v);
        }
        let ms = 0.001 * self.sr;
        let mut wet = [0.0f32; 2];
        for (k, rate) in RATES_HZ.iter().enumerate() {
            self.phase[k] = (self.phase[k] + rate / self.sr).fract();
            let s = (TAU * self.phase[k]).sin();
            let dl = (BASE_MS + DEPTH_MS * depth * s) * ms;
            let dr = (BASE_MS - DEPTH_MS * depth * s) * ms;
            wet[0] += self.lines[0].read_cubic(dl);
            wet[1] += self.lines[1].read_cubic(dr);
        }
        for l in &mut self.lines {
            l.advance();
        }
        if self.amount <= 0.0 {
            return x;
        }
        let a = self.amount;
        [x[0] * (1.0 - 0.3 * a) + wet[0] * 0.33 * a, x[1] * (1.0 - 0.3 * a) + wet[1] * 0.33 * a]
    }
}
