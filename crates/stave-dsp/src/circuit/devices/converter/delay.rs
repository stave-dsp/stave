//! An early digital delay's datapath: converters around a memory of
//! fixed-point words and a fixed-point feedback multiply.

use super::{Clock, Machine};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::fixed::{Fixed, Overflow};
use crate::units::Seconds;

/// A digital delay of the early-1980s kind: ADC, a RAM of `bits`-bit
/// words, one feedback multiply by a short coefficient, the sum
/// truncated (or rounded) and saturated (or wrapped) back to the word,
/// DAC. The output is the delayed signal alone (the dry path is analog).
///
/// # Models
/// The fixed-point datapath of an early digital delay, at the machine's
/// clock and word length ([`Machine`]):
/// `m[n] = fit(x[n] + fit(g·m[n − D] >> (c − 2)))`, out `m[n − D]`, `g` a
/// `c`-bit coefficient with `c − 2` fraction bits (gains to ±2). No specific
/// unit is claimed: the machine's rate and word length and the
/// coefficient width are the caller's.
///
/// # Derivation
/// Two's-complement arithmetic ([`Fixed`]): the product's low bits
/// dropped (toward −∞) or rounded and the product brought back to the
/// word, then the sum, each saturated or wrapped. With
/// truncation a decaying repeat cannot reach zero from below: `⌊g·(−1)⌋
/// = −1`, so the tail sticks one LSB under zero — the dead band of
/// truncated feedback.
///
/// # Verified
/// `crates/stave-dsp/src/circuit/devices/converter/tests.rs::delay_datapath_matches_an_integer_reference`
/// (bit-exact against a direct integer recursion, saturating and
/// wrapping) and
/// `crates/stave-dsp/src/circuit/devices/converter/tests.rs::truncated_feedback_sticks_one_lsb_below_zero`.
///
/// # Cost
/// Datasheet `"Digital delay 12-bit"`.
///
/// # Aliasing
/// The machine's, by design ([`Converter`](super::Converter)).
///
/// # Valid ranges
/// Delays of 1 machine sample to the length given at construction;
/// coefficient words of 2–16 bits (`|g| < 2`); machine rates up to the
/// host's.
#[derive(Clone, Debug)]
pub struct Delay {
    clock: Clock,
    word: Fixed,
    coefficient_bits: u32,
    coefficient: i32,
    memory: Vec<i32>,
    write: usize,
    length: usize,
    out: i32,
}

impl Delay {
    /// Memory for `max_seconds` at the machine's rate, allocated here;
    /// a delay of half of it, no feedback. `coefficient_bits` is clamped
    /// to 2–16.
    pub fn new(machine: Machine, max: Seconds, overflow: Overflow, coefficient_bits: u32) -> Self {
        let max_seconds = max.get();
        let capacity = (max_seconds.max(0.0) * machine.rate_hz).ceil().max(2.0) as usize;
        let bits = machine.bits.clamp(2, 32);
        Self {
            clock: Clock::new(machine),
            word: Fixed::new(bits, bits - 1, overflow, machine.rounding),
            coefficient_bits: coefficient_bits.clamp(2, 16),
            coefficient: 0,
            memory: vec![0; capacity],
            write: 0,
            length: (capacity / 2).max(1),
            out: 0,
        }
    }

    /// The delay in whole machine samples (clamped to the memory).
    pub fn set_samples(&mut self, samples: usize) {
        self.length = samples.clamp(1, self.memory.len());
    }

    pub fn set_delay(&mut self, delay: Seconds) {
        let seconds = delay.get();
        self.set_samples((seconds * self.clock.machine.rate_hz).round().max(1.0) as usize);
    }

    /// The feedback gain, held as the coefficient word holds it
    /// (truncated toward −∞, within ±2).
    pub fn set_feedback(&mut self, gain: f64) {
        let fraction = self.coefficient_bits - 2;
        let coefficient = Fixed::new(self.coefficient_bits, fraction, Overflow::Saturate, crate::primitives::quantise::Rounding::Truncate);
        self.coefficient = coefficient.code(gain);
    }

    /// The feedback gain in effect.
    pub fn feedback(&self) -> f64 {
        f64::from(self.coefficient) * 2f64.powi(-((self.coefficient_bits - 2) as i32))
    }

    /// The memory's word format.
    pub fn word(&self) -> Fixed {
        self.word
    }

    /// One machine sample of the datapath on codes: the ADC's code in,
    /// the delayed word out.
    #[inline]
    pub fn step_codes(&mut self, input: i32) -> i32 {
        let size = self.memory.len();
        let read = (self.write + size - self.length) % size;
        let delayed = self.memory[read];
        let fed = self.word.mul(delayed, self.coefficient, self.coefficient_bits - 2);
        self.memory[self.write] = self.word.add(input, fed);
        self.write = (self.write + 1) % size;
        delayed
    }
}

impl Stage for Delay {
    fn prepare(&mut self, sample_rate: f32) {
        self.clock.prepare(sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.clock.reset();
        self.memory.iter_mut().for_each(|m| *m = 0);
        self.write = 0;
        self.out = 0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let code = self.clock.sample(x).map(|c| {
            self.out = self.step_codes(c as i32);
            i64::from(self.out)
        });
        self.clock.hold(code)
    }

    fn latency(&self) -> u32 {
        self.clock.latency()
    }

    fn tail(&self) -> Tail {
        if self.coefficient == 0 {
            Tail::samples((self.length as f64 * self.clock_ratio()).ceil() as u32 + 64)
        } else {
            Tail::Infinite
        }
    }
}

impl Delay {
    /// Host samples per machine sample.
    fn clock_ratio(&self) -> f64 {
        1.0 / self.clock.ratio()
    }
}
