//! Audio converters as early digital machines had them (DSP.md H8): a
//! sample-and-hold ADC clocked at the machine's own rate, a few bits of
//! linear PCM, a zero-order-hold DAC, and the analog filters around them
//! — or their absence.
//!
//! A [`Converter`] runs inside a host at any higher rate. Each host
//! sample advances the machine's clock; when it ticks, the ADC reads
//! the (input-filtered) signal at that instant — between host samples,
//! by a 16-tap windowed sinc, so the only aliasing is the machine's —
//! quantises it, and the DAC holds the code until the next tick. The
//! hold's steps fall between host samples, and each is band-limited by
//! a minimum-phase band-limited step (minBLEP, Brandt 2001; the table the
//! oscillators use, 100 dB stopband from 0.46 of the host rate) so the
//! host rate adds no images of its own above its Nyquist; the steps' own images
//! below it, the `sinc(f/f_machine)` droop and every alias of an
//! unfiltered input are the machine's sound. [`Delay`] is the same
//! converters around a fixed-point delay memory.
//!
//! Like the bucket brigade ([`bbd`](super::bbd)), a converter is a
//! schematic part whose transfer is a sampling process no netlist
//! method solves, so it is a stage, not a netlist element.
//!
//! ```
//! use stave_dsp::circuit::devices::converter::{Converter, Machine};
//! use stave_dsp::Stage;
//!
//! let mut sp = Converter::new(Machine::SP1200);
//! sp.prepare(48_000.0);
//! let out: Vec<f32> = (0..480).map(|n| sp.tick((n as f32 * 0.05).sin() * 0.5)).collect();
//! assert!(out.iter().all(|y| y.abs() <= 0.6));
//! assert_eq!(sp.latency(), 10);
//! ```

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::primitives::osc::blep::MinBlep;
use crate::primitives::quantise::{Quantiser, Rounding};
use crate::primitives::time::interpolators::{Interpolator, Sinc};

mod delay;
pub use delay::Delay;

/// An analog filter before the ADC or after the DAC, at the host rate.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Filter {
    /// None: every alias (input) or image (output) passes.
    None,
    /// A Butterworth lowpass of `order` 2, 4, 6 or 8 (odd orders round
    /// up), corner `hz`, as cascaded biquads (bilinear, the corner
    /// prewarped).
    Butterworth { order: u32, hz: f64 },
}

/// A machine's converters.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Machine {
    /// The sampling clock, Hz.
    pub rate_hz: f64,
    /// Linear PCM word length.
    pub bits: u32,
    /// How the ADC lands between codes.
    pub rounding: Rounding,
    pub input: Filter,
    pub output: Filter,
}

impl Machine {
    /// The E-mu SP-1200's published converter spec: 12-bit linear PCM at
    /// 26.04 kHz. Its analog filters are not modelled (no schematic was
    /// consulted): set them with [`with_filters`](Self::with_filters).
    pub const SP1200: Self = Self {
        rate_hz: 26_040.0,
        bits: 12,
        rounding: Rounding::Truncate,
        input: Filter::None,
        output: Filter::None,
    };

    /// The Akai S900's published spec at its top rate: 12-bit linear PCM
    /// at 40 kHz. Filters as for [`SP1200`](Self::SP1200).
    pub const S900: Self = Self {
        rate_hz: 40_000.0,
        bits: 12,
        rounding: Rounding::Truncate,
        input: Filter::None,
        output: Filter::None,
    };

    /// The compact disc's: 16 bits at 44.1 kHz (filters as above).
    pub const CD: Self = Self {
        rate_hz: 44_100.0,
        bits: 16,
        rounding: Rounding::Nearest,
        input: Filter::None,
        output: Filter::None,
    };

    pub const fn with_rate(mut self, hz: f64) -> Self {
        self.rate_hz = hz;
        self
    }

    pub const fn with_bits(mut self, bits: u32) -> Self {
        self.bits = bits;
        self
    }

    pub const fn with_filters(mut self, input: Filter, output: Filter) -> Self {
        self.input = input;
        self.output = output;
        self
    }
}

/// Up to four biquads in series.
#[derive(Clone, Debug)]
pub(crate) struct Cascade {
    sections: [Biquad; 4],
    count: usize,
}

impl Cascade {
    pub fn new(filter: Filter) -> Self {
        let mut c = Self {
            sections: core::array::from_fn(|_| Biquad::passthrough()),
            count: 0,
        };
        c.design(filter, 48_000.0);
        c
    }

    /// The sections for `filter` at `sample_rate`: Butterworth's pole
    /// pairs, `Q_k = 1/(2·sin((2k − 1)π/(2N)))`.
    pub fn design(&mut self, filter: Filter, sample_rate: f32) {
        self.count = 0;
        let Filter::Butterworth { order, hz } = filter else {
            return;
        };
        let pairs = (order.clamp(2, 8) as usize).div_ceil(2);
        let n = 2 * pairs;
        let hz = (hz as f32).clamp(1.0, 0.49 * sample_rate);
        for k in 0..pairs {
            let angle = (2 * k + 1) as f64 * std::f64::consts::PI / (2 * n) as f64;
            let q = (1.0 / (2.0 * angle.sin())) as f32;
            let mut b = Biquad::new(FilterSpec::Lowpass { hz, q });
            b.prepare(sample_rate);
            self.sections[k] = b;
        }
        self.count = pairs;
    }

    pub fn reset(&mut self) {
        for s in &mut self.sections[..self.count] {
            s.reset();
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.sections[..self.count].iter_mut().fold(x, |y, s| s.tick(y))
    }

    /// The cascade's magnitude at `hz`.
    pub fn magnitude_at(&self, hz: f32) -> f32 {
        self.sections[..self.count].iter().map(|s| s.magnitude_at(hz)).product()
    }
}

/// Taps of the ADC's band-limited read.
const TAPS: usize = 16;

/// The machine clock and the two converters: what [`Converter`] and
/// [`Delay`] share. `sample` returns, on a tick, the ADC's code; `hold`
/// gives the DAC a code and returns the host-rate output.
#[derive(Clone, Debug)]
pub(crate) struct Clock {
    pub machine: Machine,
    pub quantiser: Quantiser,
    ratio: f64,
    phase: f64,
    /// How far back from the newest host sample the last tick fell, in
    /// samples (0 to 1).
    back: f64,
    input: Cascade,
    output: Cascade,
    history: [f32; TAPS],
    sinc: Sinc<TAPS>,
    held: f64,
    /// The naive held value over the last few samples, newest first.
    naive: [f64; 8],
    /// minBLEP corrections due, a ring from `at`.
    ring: [f64; RING],
    at: usize,
}

/// The correction ring's size (a power of two above the minBLEP's
/// length).
const RING: usize = 128;

impl Clock {
    pub fn new(machine: Machine) -> Self {
        let mut sinc = Sinc::default();
        sinc.prepare();
        let mut c = Self {
            machine,
            quantiser: Quantiser::new(machine.bits).with_rounding(machine.rounding),
            ratio: 0.5,
            phase: 0.0,
            back: 0.0,
            input: Cascade::new(machine.input),
            output: Cascade::new(machine.output),
            history: [0.0; TAPS],
            sinc,
            held: 0.0,
            naive: [0.0; 8],
            ring: [0.0; RING],
            at: 0,
        };
        c.prepare(48_000.0);
        c
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        // The machine's clock never outruns the host's.
        self.ratio = (self.machine.rate_hz / f64::from(sample_rate)).clamp(1e-6, 1.0);
        self.input.design(self.machine.input, sample_rate);
        self.output.design(self.machine.output, sample_rate);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.back = 0.0;
        self.input.reset();
        self.output.reset();
        self.history = [0.0; TAPS];
        self.held = 0.0;
        self.naive = [0.0; 8];
        self.ring = [0.0; RING];
        self.at = 0;
        self.quantiser.reset();
    }

    /// The host sample in; on a machine tick, the ADC's code.
    #[inline]
    pub fn sample(&mut self, x: f32) -> Option<i64> {
        let filtered = self.input.tick(x);
        self.history.copy_within(0..TAPS - 1, 1);
        self.history[0] = filtered;
        self.phase += self.ratio;
        if self.phase < 1.0 {
            return None;
        }
        self.phase -= 1.0;
        // The tick fell `phase / ratio` samples before this one.
        self.back = (self.phase / self.ratio).min(0.999_999);
        let read = self.sinc.interpolate(&self.history, self.back as f32);
        Some(self.quantiser.quantise_code(f64::from(read)))
    }

    /// The DAC: `code` (on a tick, else `None`) held; one host sample
    /// out. The hold's step sits `back` before this sample and is
    /// band-limited by the minBLEP residual (`len` samples of it, from
    /// this one on), against the naive hold delayed by the table's delay.
    #[inline]
    pub fn hold(&mut self, code: Option<i64>) -> f32 {
        let table = MinBlep::get();
        if let Some(code) = code {
            let value = code as f64 * self.quantiser.step();
            let dv = value - self.held;
            self.held = value;
            for k in 0..table.len {
                let (step, _) = table.residual(self.back + k as f64);
                self.ring[(self.at + k) & (RING - 1)] += dv * step;
            }
        }
        self.naive.copy_within(0..7, 1);
        self.naive[0] = self.held;
        let out = self.naive[table.delay] + self.ring[self.at];
        self.ring[self.at] = 0.0;
        self.at = (self.at + 1) & (RING - 1);
        self.output.tick(out as f32)
    }

    /// Host samples from input to output: the sinc's look-ahead and the
    /// minBLEP's delay.
    pub fn latency(&self) -> u32 {
        (Sinc::<TAPS>::NEWER + MinBlep::get().delay) as u32
    }

    pub fn output_filter(&self) -> &Cascade {
        &self.output
    }

    pub fn input_filter(&self) -> &Cascade {
        &self.input
    }

    /// Machine samples per host sample.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }
}

/// An early digital machine's converters: ADC at the machine's clock,
/// `bits` of linear PCM, a zero-order-hold DAC, and the analog filters
/// around them.
///
/// # Models
/// A sampler's or digital delay's conversion: sample-and-hold at the
/// machine's rate (any rate up to the host's, odd ones included), a
/// linear two's-complement ADC of the machine's word length (truncating
/// or rounding), a zero-order-hold DAC, and optional Butterworth
/// anti-alias and reconstruction filters — absent by default, as on
/// machines that shipped without them. Parts: [`Machine::SP1200`] (12-bit,
/// 26.04 kHz), [`Machine::S900`] (12-bit, 40 kHz), [`Machine::CD`].
///
/// # Derivation
/// Sampling theory: a tone at `f` sampled at `F` appears at `|f − kF|`
/// for the `k` that folds it below `F/2`; the hold's spectrum is the
/// sampled one times `sinc(f/F)`, with images at `kF ± f`. The ADC reads
/// between host samples by a Kaiser-windowed sinc
/// ([`Sinc`]), the hold's
/// steps are band-limited by minBLEP residuals, the word is the
/// [`Quantiser`]'s.
///
/// # Verified
/// `crates/stave-test/tests/converters_match_theory.rs::aliases_land_where_sampling_theory_puts_them`
/// (an unfiltered tone above the machine's Nyquist at `F − f`, its level
/// the hold's `sinc` within 0.02 dB, measured 0.004; in-band droop the
/// same), `crates/stave-test/tests/converters_match_theory.rs::hold_images_follow_the_sinc`
/// (images at `F − f` to 20 kHz on the `sinc` within 0.1 dB, measured
/// 0.013 at 20 kHz),
/// `crates/stave-test/tests/converters_match_theory.rs::the_word_holds_its_codes`
/// (a held input lands on its truncated code exactly; over full scale, the
/// top code) and
/// `crates/stave-test/tests/converters_match_theory.rs::anti_alias_filters_remove_what_they_should`
/// (an alias and an image fall by their filter's magnitude within 0.5
/// dB).
///
/// # Cost
/// Datasheet `"Converter SP-1200"`.
///
/// # Aliasing
/// The machine's own, by design (the point of the model). The host rate
/// adds little: the input is read band-limited (the windowed sinc's
/// stopband) and the hold's steps are minBLEP-corrected (100 dB down
/// from 0.46 of the host rate).
///
/// # Valid ranges
/// Machine rates up to the host rate (higher is clamped to it); 2–32
/// bits; a latency of 10 host samples ([`Stage::latency`]).
#[derive(Clone, Debug)]
pub struct Converter {
    clock: Clock,
}

impl Converter {
    pub fn new(machine: Machine) -> Self {
        Self {
            clock: Clock::new(machine),
        }
    }

    pub fn machine(&self) -> Machine {
        self.clock.machine
    }

    /// The anti-alias filter's magnitude at `hz` (1 with none).
    pub fn input_magnitude_at(&self, hz: f32) -> f32 {
        self.clock.input_filter().magnitude_at(hz)
    }

    /// The reconstruction filter's magnitude at `hz` (1 with none).
    pub fn output_magnitude_at(&self, hz: f32) -> f32 {
        self.clock.output_filter().magnitude_at(hz)
    }
}

impl Stage for Converter {
    fn prepare(&mut self, sample_rate: f32) {
        self.clock.prepare(sample_rate);
    }

    fn reset(&mut self) {
        self.clock.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let code = self.clock.sample(x);
        self.clock.hold(code)
    }

    fn latency(&self) -> u32 {
        self.clock.latency()
    }

    fn tail(&self) -> Tail {
        Tail::samples(64)
    }
}

#[cfg(test)]
mod tests;
