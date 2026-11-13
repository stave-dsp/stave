//! The Karplus–Strong string: a tuned delay loop with a loss filter.

use super::{capacity, loop_gain, tune};
use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::random::Xorshift32;
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};
use crate::units::{Hz, Seconds};

/// A plucked or bowed string: a delay loop one period long whose
/// one-pole loss filter makes high partials die faster than the
/// fundamental.
///
/// # Models
///
/// Karplus & Strong's plucked string (1983) as Jaffe & Smith extended
/// it (1983) and Smith's digital waveguide reads it (1992): a lossless
/// string's round trip as one delay line, its losses lumped into a loop
/// filter, its tuning made exact by a fractional delay (S2's
/// interpolator `I`). The input drives the string (a bow, an exciter,
/// or [`pluck`](Self::pluck)'s noise burst); the output is the loop.
///
/// # Derivation
///
/// The loop filter is `H(z) = g(1 − a)/(1 − a·z⁻¹)`. Its magnitude at
/// the fundamental `ω₀` is the loop gain `10^(−3/(T₀·f₀))` for the
/// fundamental's T60 `T₀`, and at the reference `ω₁` (the 8th harmonic,
/// held below 0.45·fs) the gain for the treble T60 `T₁`. Their ratio
/// `ρ` fixes the pole: `a² + 2βa + 1 = 0` with
/// `β = (cos ω₀ − ρ² cos ω₁)/(ρ² − 1)`, the root inside the unit
/// circle; `g` then sets the fundamental's level (held below 1, so the
/// DC loop decays). The filter's phase delay at `ω₀`,
/// `atan2(a sin ω₀, 1 − a cos ω₀)/ω₀`, and the interpolator's own phase
/// delay and magnitude at `ω₀` and `ω₁`
/// ([`Interpolator::response`]) come off the line's length and into the
/// gains, solved together by fixed-point iteration, so the loop is one
/// period at the fundamental exactly whatever the read (Aeolian's
/// strings compensated the low-frequency limit `a/(1 − a)` and assumed
/// an ideal read; the exact delay matters toward the top of the range,
/// and with Thiran or a table-driven sinc anywhere).
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/resonators/tests.rs::karplus_strong_is_in_tune`:
/// the measured fundamental from 41 Hz to 2.6 kHz (E1–E7) at 48 kHz
/// within 0.1 cent with every interpolator (`Linear`, `Lagrange3`,
/// `Lagrange5`, `Thiran`, `Sinc<8>`: 0.026–0.055 measured; uncompensated,
/// `Thiran` was 0.95 cent and `Sinc<8>` 0.14).
/// `crates/stave-dsp/src/primitives/resonators/tests.rs::karplus_strong_decays_as_set`:
/// the measured T60 of the fundamental (within 0.05 %) and of the 8th
/// harmonic (over its first 30 dB) within 1.5 % of their settings,
/// 0.2–8 s.
///
/// # Cost
///
/// `"KarplusStrong<Lagrange3>"` on the datasheet.
///
/// # Aliasing
///
/// None: linear (a pluck's noise burst is white, unfiltered).
///
/// # Valid ranges
///
/// Pitch from the `lowest` given at construction to about 0.2·fs (the
/// loop needs a sample or two of line); treble T60 ≤ fundamental T60
/// (longer is held equal). A fundamental T60 so long that `g` would
/// reach 1 is held just below, and decays a little sooner.
///
/// ```
/// use stave_dsp::primitives::resonators::KarplusStrong;
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// let mut string = KarplusStrong::<stave_dsp::primitives::time::interpolators::Lagrange3>::new(hz(40.0), hz(110.0));
/// string.set_decay(seconds(3.0), seconds(0.4));
/// string.prepare(48_000.0);
/// string.pluck(0.8);
/// let ring: f32 = (0..48_000).map(|_| string.tick(0.0).abs()).sum();
/// assert!(ring > 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct KarplusStrong<I: Interpolator = Lagrange3> {
    line: DelayLine<I>,
    lowest: f32,
    pitch: f32,
    decay: [f64; 2],
    sample_rate: f32,
    delay: f32,
    gain: f32,
    pole: f32,
    state: f32,
    noise: Xorshift32,
    burst: u32,
    burst_level: f32,
}

/// The treble T60 is set at this harmonic.
const TREBLE_HARMONIC: f64 = 8.0;

impl<I: Interpolator> KarplusStrong<I> {
    /// A string that can tune down to `lowest`, at `pitch`, fundamental
    /// and treble T60 both 2 s. The line is allocated in `prepare`.
    pub fn new(lowest: Hz, pitch: Hz) -> Self {
        let mut s = Self {
            line: DelayLine::default(),
            lowest: lowest.get(),
            pitch: pitch.get(),
            decay: [2.0, 2.0],
            sample_rate: 48_000.0,
            delay: 1.0,
            gain: 0.0,
            pole: 0.0,
            state: 0.0,
            noise: Xorshift32::new(0x5EED_0A11),
            burst: 0,
            burst_level: 0.0,
        };
        s.update();
        s
    }

    /// Retune (realtime; the loop's contents carry on at the new length).
    pub fn set_pitch(&mut self, pitch: Hz) {
        self.pitch = pitch.get();
        self.update();
    }

    /// The fundamental's T60, and the treble's (at the 8th harmonic).
    pub fn set_decay(&mut self, fundamental: Seconds, treble: Seconds) {
        self.decay = [fundamental.get(), treble.get()];
        self.update();
    }

    /// Strike: a burst of white noise one period long at `level`, added
    /// to the input (the classic Karplus–Strong excitation).
    pub fn pluck(&mut self, level: f32) {
        self.burst = self.delay.round().max(1.0) as u32;
        self.burst_level = level;
    }

    /// The loop filter's pole `a` and gain `g`, and the line's length in
    /// samples, at the prepared rate.
    pub fn loop_filter(&self) -> (f32, f32, f32) {
        (self.pole, self.gain, self.delay)
    }

    fn update(&mut self) {
        let fs = f64::from(self.sample_rate);
        let f0 = f64::from(self.pitch).clamp(f64::from(self.lowest), 0.45 * fs);
        let period = fs / f0;
        let w0 = core::f64::consts::TAU * f0 / fs;
        let w1 = (TREBLE_HARMONIC * w0).min(0.9 * core::f64::consts::PI);
        let g0 = loop_gain(1.0 / f0, self.decay[0]);
        let g1 = loop_gain(1.0 / f0, self.decay[1].min(self.decay[0]));
        let treble = (w1 > 1.5 * w0).then_some((w1, g1));
        let t = tune(self.line.interpolator(), period, w0, g0, treble);
        self.pole = t.pole as f32;
        self.gain = t.gain as f32;
        self.delay = t.delay;
    }
}

impl<I: Interpolator> Stage for KarplusStrong<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.line.allocate(capacity(self.lowest, sample_rate, I::OLDER));
        self.update();
        self.reset();
    }
    fn reset(&mut self) {
        self.line.reset();
        self.state = 0.0;
        self.burst = 0;
        self.noise = Xorshift32::new(0x5EED_0A11);
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut drive = x;
        if self.burst > 0 {
            self.burst -= 1;
            drive += self.burst_level * self.noise.next_bipolar();
        }
        let y = self.line.interpolated(self.delay);
        let a = self.pole;
        self.state = denormal::flush(self.gain * (1.0 - a) * y + a * self.state);
        let w = denormal::finite_or_zero(drive + self.state);
        self.line.write(w);
        self.line.advance();
        w
    }
    /// The fundamental's T60 (the slowest partial), as 120 dB.
    fn tail(&self) -> Tail {
        let g = f64::from(self.gain);
        if g <= 0.0 {
            return Tail::samples(self.delay.ceil() as u32 + 1);
        }
        Tail::from_pole(g.powf(1.0 / f64::from(self.delay.max(1.0)))).plus(self.delay.ceil() as u32)
    }
}
