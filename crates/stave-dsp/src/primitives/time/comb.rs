//! Schroeder's recirculating delays: the feedback [`Comb`] and the
//! [`DelayAllpass`] — the two building blocks of every classic
//! algorithmic reverb, and of resonators and diffusers generally.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;

/// Whole samples in `ms` at `sample_rate`, at least one (a recirculating
/// delay of zero is a zero-delay loop). Schroeder's elements recirculate
/// at integer lengths: an interpolated read inside the loop is a lowpass
/// there, and breaks the allpass's flat magnitude.
fn delay_samples(ms: f32, sample_rate: f32) -> usize {
    ((ms * 0.001 * sample_rate).round() as usize).max(1)
}

/// The decay of a loop that scales by `gain` once every `delay` samples.
fn loop_tail(gain: f32, delay: usize) -> Tail {
    let g = f64::from(gain.abs());
    let d = delay as u32;
    if g == 0.0 {
        return Tail::samples(d);
    }
    // gain^(1/delay) per sample, plus the first pass through the line.
    Tail::from_pole(g.powf(1.0 / f64::from(d))).plus(d)
}

/// Line length for `max_ms` at `sample_rate`.
fn capacity(max_ms: f32, sample_rate: f32) -> usize {
    (max_ms * 0.001 * sample_rate).ceil() as usize + 1
}

/// Feedback comb: `y[n] = x[n−N] + g·lp(y)[n−N]`, the output being the
/// delayed signal. `damping` puts a one-pole lowpass in the loop (0 =
/// none, toward 1 = darker), so highs decay faster than lows as in a
/// real room.
#[derive(Clone, Debug)]
pub struct Comb {
    line: DelayLine,
    max_ms: f32,
    delay_ms: f32,
    feedback: f32,
    damping: f32,
    sample_rate: f32,
    delay: usize,
    lp: f32,
}

impl Comb {
    /// A comb that can reach `max_ms` of delay, set to `delay_ms` with
    /// loop gain `feedback`. The line is allocated in `prepare`.
    pub fn new(max_ms: f32, delay_ms: f32, feedback: f32) -> Self {
        let mut c = Self {
            line: DelayLine::new(),
            max_ms,
            delay_ms,
            feedback,
            damping: 0.0,
            sample_rate: 48_000.0,
            delay: 1,
            lp: 0.0,
        };
        c.set_delay_ms(delay_ms);
        c
    }

    /// Loop length (rounded to whole samples), held inside
    /// `[1 sample, max_ms]`.
    pub fn set_delay_ms(&mut self, ms: f32) {
        self.delay_ms = ms.min(self.max_ms);
        self.delay = delay_samples(self.delay_ms, self.sample_rate);
    }

    /// The loop length in whole samples at the prepared rate.
    pub fn delay_samples(&self) -> usize {
        self.delay
    }

    /// Loop gain; `|feedback| < 1` decays.
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback;
    }

    /// In-loop lowpass, 0 (bright) … 1 (dark).
    pub fn set_damping(&mut self, damping: f32) {
        debug_assert!((0.0..1.0).contains(&damping), "damping {damping}");
        self.damping = damping;
    }
}

impl Stage for Comb {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.line.allocate(capacity(self.max_ms, sample_rate));
        self.set_delay_ms(self.delay_ms);
        self.reset();
    }
    fn reset(&mut self) {
        self.line.reset();
        self.lp = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let out = self.line.read_int(self.delay);
        self.lp = denormal::flush(out + (self.lp - out) * self.damping);
        self.line.write(x + self.feedback * self.lp);
        self.line.advance();
        out
    }
    fn tail(&self) -> Tail {
        loop_tail(self.feedback, self.delay)
    }
}

/// Schroeder allpass: `y[n] = −g·x[n] + x[n−N] + g·y[n−N]`. A flat
/// magnitude with a dense, delay-spread phase — the diffuser.
#[derive(Clone, Debug)]
pub struct DelayAllpass {
    line: DelayLine,
    max_ms: f32,
    delay_ms: f32,
    gain: f32,
    sample_rate: f32,
    delay: usize,
}

impl DelayAllpass {
    /// An allpass that can reach `max_ms` of delay, set to `delay_ms`
    /// with coefficient `gain` (|gain| < 1). The line is allocated in
    /// `prepare`.
    pub fn new(max_ms: f32, delay_ms: f32, gain: f32) -> Self {
        let mut a = Self {
            line: DelayLine::new(),
            max_ms,
            delay_ms,
            gain,
            sample_rate: 48_000.0,
            delay: 1,
        };
        a.set_delay_ms(delay_ms);
        a
    }

    /// Loop length (rounded to whole samples), held inside
    /// `[1 sample, max_ms]`.
    pub fn set_delay_ms(&mut self, ms: f32) {
        self.delay_ms = ms.min(self.max_ms);
        self.delay = delay_samples(self.delay_ms, self.sample_rate);
    }

    pub fn set_gain(&mut self, gain: f32) {
        debug_assert!(gain.abs() < 1.0, "allpass gain {gain}");
        self.gain = gain;
    }
}

impl Stage for DelayAllpass {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.line.allocate(capacity(self.max_ms, sample_rate));
        self.set_delay_ms(self.delay_ms);
        self.reset();
    }
    fn reset(&mut self) {
        self.line.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let delayed = self.line.read_int(self.delay);
        let v = denormal::flush(x + self.gain * delayed);
        self.line.write(v);
        self.line.advance();
        delayed - self.gain * v
    }
    fn tail(&self) -> Tail {
        loop_tail(self.gain, self.delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(stage: &mut impl Stage, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| stage.tick(if i == 0 { 1.0 } else { 0.0 }))
            .collect()
    }

    #[test]
    fn comb_echoes_every_delay_scaled_by_feedback() {
        // 1 ms at 48 kHz = 48 samples.
        let mut c = Comb::new(10.0, 1.0, 0.5);
        c.prepare(48_000.0);
        let h = impulse(&mut c, 200);
        for (i, &y) in h.iter().enumerate() {
            let want = match i {
                48 => 1.0,
                96 => 0.5,
                144 => 0.25,
                192 => 0.125,
                _ => 0.0,
            };
            assert!((y - want).abs() < 1e-6, "sample {i}: {y} want {want}");
        }
    }

    #[test]
    fn delay_allpass_is_flat() {
        let mut a = DelayAllpass::new(10.0, 1.3, 0.7);
        a.prepare(48_000.0);
        // An allpass preserves energy: Σh² = 1.
        let energy: f64 = impulse(&mut a, 20_000)
            .iter()
            .map(|&y| f64::from(y) * f64::from(y))
            .sum();
        assert!((energy - 1.0).abs() < 1e-4, "Σh² = {energy}");
    }

    #[test]
    fn recirculating_tails_cover_the_measured_decay() {
        let mut c = Comb::new(50.0, 7.0, 0.8);
        c.prepare(48_000.0);
        let Tail::Samples(n) = c.tail() else { panic!() };
        let h = impulse(&mut c, 2 * n as usize);
        let last_loud = h.iter().rposition(|y| y.abs() > 1e-6).unwrap() as u32;
        assert!(
            last_loud <= n && last_loud > n * 9 / 10,
            "rang {last_loud}, reported {n}"
        );
        let mut a = DelayAllpass::new(50.0, 7.0, 0.8);
        a.prepare(48_000.0);
        let Tail::Samples(n) = a.tail() else { panic!() };
        let h = impulse(&mut a, 2 * n as usize);
        let last_loud = h.iter().rposition(|y| y.abs() > 1e-6).unwrap() as u32;
        assert!(last_loud <= n, "rang {last_loud}, reported {n}");
        assert_eq!(Comb::new(10.0, 1.0, 1.0).tail(), Tail::Infinite);
    }
}
