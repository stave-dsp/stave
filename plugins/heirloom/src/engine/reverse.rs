//! `ReverseLine` — a delay line that can be read BACKWARD. Two heads
//! sweep away from the write head at twice the tape speed, so each
//! plays the last `len` ticks in reverse; they run half a segment
//! apart under sin² windows that sum to one, so the splices are
//! silent and the reverser adds no loop gain. The same line also
//! serves a plain forward tap — the X knob of program I blends the
//! two, and program V uses it as an ordinary delay.

use stave_dsp::DelayLine;

pub(super) struct ReverseLine {
    line: DelayLine,
    /// Sweep phase 0..1 through the current segment (head 0; head 1
    /// rides half a segment behind).
    phase: f32,
}

impl ReverseLine {
    pub(super) fn new() -> Self {
        Self { line: DelayLine::new(), phase: 0.0 }
    }

    /// Size for segments up to `max_len` ticks. `offset` staggers the
    /// sweep (the right lane runs a quarter segment off the left, so
    /// the two ears splice at different instants — width for free).
    pub(super) fn prepare(&mut self, max_len: usize, offset: f32) {
        self.line.prepare(2 * max_len + 8);
        self.phase = offset;
    }

    pub(super) fn reset(&mut self, offset: f32) {
        self.line.reset();
        self.phase = offset;
    }

    /// The reversed read for a `len`-tick segment. Advances the sweep.
    #[inline]
    pub(super) fn reverse(&mut self, len: f32) -> f32 {
        let p0 = self.phase;
        let p1 = if p0 >= 0.5 { p0 - 0.5 } else { p0 + 0.5 };
        let g0 = sin2(p0);
        let y = g0 * self.line.read(1.0 + 2.0 * len * p0)
            + (1.0 - g0) * self.line.read(1.0 + 2.0 * len * p1);
        self.phase += 1.0 / len.max(1.0);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        y
    }

    /// The ordinary forward tap, `d` ticks back.
    #[inline]
    pub(super) fn forward(&self, d: f32) -> f32 {
        self.line.read(d)
    }

    #[inline]
    pub(super) fn write(&mut self, x: f32) {
        self.line.write(x);
        self.line.advance();
    }
}

/// sin²(πp): 0 at the splice, 1 mid-segment; sin²(πp) + sin²(π(p+½)) = 1.
#[inline]
pub(super) fn sin2(p: f32) -> f32 {
    let s = (core::f32::consts::PI * p).sin();
    s * s
}
