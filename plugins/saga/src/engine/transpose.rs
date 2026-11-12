//! `Transposer` — the classic two-head delay-line pitch shifter, the
//! sound of a 32 kHz pedal's octave: a tap whose delay sweeps at
//! 1 − ratio, a twin half a window behind, sin² gains (summing to
//! one) hiding each wrap. Stereo, one shared sweep, so the seams
//! never wander the image. Ratio may change every tick — program V
//! steps it through its interval sequence.

use stave_dsp::DelayLine;

use super::reverse::sin2;

pub(super) struct Transposer {
    lines: [DelayLine; 2],
    phase: f32,
    window: f32,
}

impl Transposer {
    /// `window` in ticks — the grain. Longer reads smoother on
    /// octave-down, shorter tighter on octave-up.
    pub(super) fn new(window: f32) -> Self {
        Self { lines: [DelayLine::new(), DelayLine::new()], phase: 0.0, window }
    }

    pub(super) fn prepare(&mut self) {
        for l in &mut self.lines {
            l.prepare(self.window as usize + 8);
        }
        self.phase = 0.0;
    }

    pub(super) fn reset(&mut self) {
        for l in &mut self.lines {
            l.reset();
        }
        self.phase = 0.0;
    }

    #[inline]
    pub(super) fn process(&mut self, x: [f32; 2], ratio: f32) -> [f32; 2] {
        let w = self.window;
        let p0 = self.phase;
        let p1 = if p0 >= 0.5 { p0 - 0.5 } else { p0 + 0.5 };
        let g0 = sin2(p0);
        let (d0, d1) = (1.0 + p0 * w, 1.0 + p1 * w);
        let mut y = [0.0; 2];
        for (c, line) in self.lines.iter_mut().enumerate() {
            line.write(x[c]);
            line.advance();
            y[c] = g0 * line.read(d0) + (1.0 - g0) * line.read(d1);
        }
        self.phase = (self.phase + (1.0 - ratio) / w).rem_euclid(1.0);
        y
    }
}
