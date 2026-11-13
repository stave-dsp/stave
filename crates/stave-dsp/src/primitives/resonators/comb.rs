//! The pitched comb resonator.

use super::{capacity, loop_gain, tune};
use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};
use crate::units::{Hz, Seconds};

/// A comb resonator: a feedback comb tuned to a pitch, ringing every
/// harmonic of it for one T60.
///
/// # Models
///
/// The feedback comb `y[n] = x[n] + g·y[n − P]`, `P = fs/f₀` samples
/// read through S2's fractional-delay interpolator `I`: resonances at
/// every multiple of `f₀`, all decaying alike — the harmonic resonator
/// of sympathetic-string and "resonator" effects. Unlike
/// [`time::comb::Comb`](crate::primitives::time::comb::Comb), the
/// reverb building block (integer lengths in ms), it is tuned in Hz.
///
/// # Derivation
///
/// `g = 10^(−3/(T60·f₀))`, so every resonance falls 60 dB in T60; the
/// line's length less the interpolator's phase delay at `f₀`, and `g`
/// over its magnitude there ([`Interpolator::response`]), keep the
/// fundamental exact whatever the read. The
/// output is scaled by `1 − g`, the inverse of the resonances' peak
/// gain `1/(1 − g)`, so a peak passes at unity whatever the decay.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/resonators/tests.rs::comb_is_in_tune_and_decays_as_set`:
/// the measured fundamental E1–E7 at 48 kHz within 0.1 cent
/// (`Lagrange3`, 0.013 measured), and the measured T60 within 1 % of the
/// setting, 0.2–8 s (0.04 % measured); a resonance peaks at 0 dB within
/// 0.01 dB.
///
/// # Cost
///
/// `"Comb resonator<Lagrange3>"` on the datasheet.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// Pitch from the `lowest` given at construction to about 0.2·fs;
/// T60 > 0 (0 passes the input through).
///
/// ```
/// use stave_dsp::primitives::resonators::Comb;
/// use stave_dsp::primitives::time::interpolators::Lagrange3;
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// let mut ring = Comb::<Lagrange3>::new(hz(50.0), hz(220.0), seconds(1.5));
/// ring.prepare(48_000.0);
/// let y = ring.tick(1.0);
/// assert!(y > 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct Comb<I: Interpolator = Lagrange3> {
    line: DelayLine<I>,
    lowest: f32,
    pitch: f32,
    decay: f64,
    sample_rate: f32,
    delay: f32,
    gain: f32,
    scale: f32,
}

impl<I: Interpolator> Comb<I> {
    /// A comb that can tune down to `lowest`, at `pitch`, ringing for
    /// `decay` (T60). The line is allocated in `prepare`.
    pub fn new(lowest: Hz, pitch: Hz, decay: Seconds) -> Self {
        let mut c = Self {
            line: DelayLine::default(),
            lowest: lowest.get(),
            pitch: pitch.get(),
            decay: decay.get(),
            sample_rate: 48_000.0,
            delay: 1.0,
            gain: 0.0,
            scale: 1.0,
        };
        c.update();
        c
    }

    /// Retune (realtime).
    pub fn set_pitch(&mut self, pitch: Hz) {
        self.pitch = pitch.get();
        self.update();
    }

    /// The T60 of every resonance.
    pub fn set_decay(&mut self, decay: Seconds) {
        self.decay = decay.get();
        self.update();
    }

    fn update(&mut self) {
        let fs = f64::from(self.sample_rate);
        let f0 = f64::from(self.pitch).clamp(f64::from(self.lowest), 0.45 * fs);
        let g0 = loop_gain(1.0 / f0, self.decay);
        let w0 = core::f64::consts::TAU * f0 / fs;
        let t = tune(self.line.interpolator(), fs / f0, w0, g0, None);
        self.gain = t.gain as f32;
        self.delay = t.delay;
        self.scale = (1.0 - g0) as f32;
    }
}

impl<I: Interpolator> Stage for Comb<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.line.allocate(capacity(self.lowest, sample_rate, I::OLDER));
        self.update();
        self.reset();
    }
    fn reset(&mut self) {
        self.line.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.line.interpolated(self.delay);
        let w = denormal::finite_or_zero(denormal::flush(x + self.gain * y));
        self.line.write(w);
        self.line.advance();
        self.scale * w
    }
    fn tail(&self) -> Tail {
        let g = f64::from(self.gain);
        if g <= 0.0 {
            return Tail::None;
        }
        Tail::from_pole(g.powf(1.0 / f64::from(self.delay.max(1.0)))).plus(self.delay.ceil() as u32)
    }
}
