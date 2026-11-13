//! Fixed-point arithmetic as early digital hardware did it (DSP.md H8):
//! two's-complement words of a few bits, products truncated or rounded
//! back to the word, sums that saturate or wrap.
//!
//! A [`Fixed`] format is `bits` wide with `fraction` of them after the
//! point (Q`(bits − fraction)`.`fraction`, sign included in the integer
//! part), so a 12-bit sampler word holding ±1 is `Fixed::new(12, 11, …)`.
//! Codes are `i32`; every operation is exact integer arithmetic, then
//! the format's [`Rounding`] and [`Overflow`].
//!
//! ```
//! use stave_dsp::primitives::fixed::{Fixed, Overflow};
//! use stave_dsp::primitives::quantise::Rounding;
//!
//! let word = Fixed::new(12, 11, Overflow::Wrap, Rounding::Truncate);
//! let a = word.code(0.75);
//! assert_eq!(a, 1536);
//! // 0.75 + 0.75 wraps past +1 to −0.5, as a 12-bit adder does.
//! assert_eq!(word.value(word.add(a, a)), -0.5);
//! // A 0.5 gain in an 8-bit Q1.7 coefficient, truncated back to 12 bits.
//! assert_eq!(word.mul(a, 64, 7), 768);
//! ```

use crate::primitives::quantise::Rounding;

/// What a result beyond the word does.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Clamp to the word's ends (a saturating adder).
    Saturate,
    /// Keep the low bits: two's-complement wraparound, a full-scale
    /// positive sum turning negative.
    Wrap,
}

/// A two's-complement fixed-point format and the arithmetic on it.
///
/// # Models
/// The datapath of fixed-point audio hardware (12-bit samplers, early
/// digital delays and reverbs): words of `bits` with `fraction` bits
/// after the point, sums and products brought back to the word by
/// truncation (dropping low bits, toward −∞) or rounding, and overflow
/// that saturates or wraps.
///
/// # Derivation
/// Two's-complement integer arithmetic: a product of codes carries the
/// sum of the operands' fraction bits and is shifted right by the
/// coefficient's (arithmetic shift = floor, or floor after adding half
/// an LSB to round); overflow keeps the low `bits` bits (wrap) or
/// clamps.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/fixed/tests.rs::operations_match_wide_integer_arithmetic`
/// (every operation bit-exact against an `i128` reference over random
/// operands and formats, 2–32 bits) and
/// `crates/stave-dsp/src/primitives/fixed/tests.rs::conversion_errors_stay_within_a_step`.
///
/// # Cost
/// Datasheet `"Digital delay 12-bit"` (the format inside a delay's
/// feedback path, one multiply and one add a sample).
///
/// # Aliasing
/// None of its own: integer arithmetic at the rate it runs; truncation
/// error is signal-correlated (harmonics, limit cycles), the hardware's
/// sound.
///
/// # Valid ranges
/// `bits` 2–32, `fraction` below `bits`; coefficient fractions 0–30.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fixed {
    bits: u32,
    fraction: u32,
    overflow: Overflow,
    rounding: Rounding,
}

impl Fixed {
    /// `bits` is clamped to 2–32 and `fraction` below it.
    pub const fn new(bits: u32, fraction: u32, overflow: Overflow, rounding: Rounding) -> Self {
        let bits = if bits < 2 {
            2
        } else if bits > 32 {
            32
        } else {
            bits
        };
        let fraction = if fraction >= bits { bits - 1 } else { fraction };
        Self {
            bits,
            fraction,
            overflow,
            rounding,
        }
    }

    pub const fn bits(&self) -> u32 {
        self.bits
    }

    pub const fn fraction(&self) -> u32 {
        self.fraction
    }

    /// The smallest and largest codes.
    pub const fn range(&self) -> (i64, i64) {
        let half = 1i64 << (self.bits - 1);
        (-half, half - 1)
    }

    /// The value of one code step.
    pub fn lsb(&self) -> f64 {
        2f64.powi(-(self.fraction as i32))
    }

    /// A wide integer brought into the word by the overflow rule.
    #[inline]
    pub fn fit(&self, wide: i64) -> i32 {
        let (lo, hi) = self.range();
        match self.overflow {
            Overflow::Saturate => wide.clamp(lo, hi) as i32,
            Overflow::Wrap => {
                let shift = 64 - self.bits;
                ((wide << shift) >> shift) as i32
            }
        }
    }

    /// `wide` shifted right by `shift` bits under the rounding rule.
    #[inline]
    fn shift_down(&self, wide: i64, shift: u32) -> i64 {
        if shift == 0 {
            return wide;
        }
        match self.rounding {
            Rounding::Truncate => wide >> shift,
            Rounding::Nearest => (wide + (1i64 << (shift - 1))) >> shift,
        }
    }

    /// The code of `x` (rounded or truncated, then overflowed; NaN is
    /// zero).
    #[inline]
    pub fn code(&self, x: f64) -> i32 {
        let scaled = x * 2f64.powi(self.fraction as i32);
        let whole = match self.rounding {
            Rounding::Truncate => scaled.floor(),
            Rounding::Nearest => (scaled + 0.5).floor(),
        };
        if whole.is_nan() {
            return 0;
        }
        // Beyond i64 the wrap is meaningless: saturate there first.
        self.fit(whole.clamp(-9.0e18, 9.0e18) as i64)
    }

    /// A code's value.
    #[inline]
    pub fn value(&self, code: i32) -> f64 {
        f64::from(code) * self.lsb()
    }

    #[inline]
    pub fn add(&self, a: i32, b: i32) -> i32 {
        self.fit(i64::from(a) + i64::from(b))
    }

    #[inline]
    pub fn sub(&self, a: i32, b: i32) -> i32 {
        self.fit(i64::from(a) - i64::from(b))
    }

    /// `a` times a coefficient `coefficient` with `coefficient_fraction`
    /// fraction bits, back in this format: the double-width product
    /// shifted down by the coefficient's fraction bits under the
    /// rounding rule, then overflowed.
    #[inline]
    pub fn mul(&self, a: i32, coefficient: i32, coefficient_fraction: u32) -> i32 {
        let wide = i64::from(a) * i64::from(coefficient);
        self.fit(self.shift_down(wide, coefficient_fraction.min(62)))
    }

    /// A word of this format moved into `to` (its fraction bits shifted,
    /// by `to`'s rounding when it has fewer, then `to`'s overflow).
    #[inline]
    pub fn convert(&self, code: i32, to: &Fixed) -> i32 {
        let wide = i64::from(code);
        let moved = if to.fraction >= self.fraction {
            wide << (to.fraction - self.fraction)
        } else {
            to.shift_down(wide, self.fraction - to.fraction)
        };
        to.fit(moved)
    }
}

#[cfg(test)]
mod tests;
