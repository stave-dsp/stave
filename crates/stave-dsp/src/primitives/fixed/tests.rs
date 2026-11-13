use super::*;
use crate::primitives::random::SplitMix64;

/// The reference: exact wide arithmetic, floor or round-half-up, then
/// clamp or reduce modulo 2^bits into the signed range.
fn reference_fit(bits: u32, overflow: Overflow, wide: i128) -> i128 {
    let half = 1i128 << (bits - 1);
    match overflow {
        Overflow::Saturate => wide.clamp(-half, half - 1),
        Overflow::Wrap => (wide + half).rem_euclid(2 * half) - half,
    }
}

fn reference_shift(rounding: Rounding, wide: i128, shift: u32) -> i128 {
    if shift == 0 {
        return wide;
    }
    let d = 1i128 << shift;
    match rounding {
        Rounding::Truncate => wide.div_euclid(d),
        Rounding::Nearest => (wide + d / 2).div_euclid(d),
    }
}

#[test]
fn operations_match_wide_integer_arithmetic() {
    let mut r = SplitMix64::new(77);
    for _ in 0..20_000 {
        let bits = 2 + (r.next_u64() % 31) as u32;
        let fraction = (r.next_u64() % u64::from(bits)) as u32;
        let overflow = if r.next_u64() % 2 == 0 { Overflow::Saturate } else { Overflow::Wrap };
        let rounding = if r.next_u64() % 2 == 0 { Rounding::Truncate } else { Rounding::Nearest };
        let f = Fixed::new(bits, fraction, overflow, rounding);
        let (lo, hi) = f.range();
        let mut operand = || (lo + (r.next_u64() % ((hi - lo + 1) as u64)) as i64) as i32;
        let (a, b) = (operand(), operand());
        let fit = |w: i128| reference_fit(bits, overflow, w) as i32;
        assert_eq!(f.add(a, b), fit(i128::from(a) + i128::from(b)), "{f:?} {a}+{b}");
        assert_eq!(f.sub(a, b), fit(i128::from(a) - i128::from(b)), "{f:?} {a}-{b}");
        let cf = (r.next_u64() % 31) as u32;
        let c = (r.next_u64() as i64 >> 33) as i32;
        let product = reference_shift(rounding, i128::from(a) * i128::from(c), cf);
        assert_eq!(f.mul(a, c, cf), fit(product), "{f:?} {a}*{c}>>{cf}");
        let to = Fixed::new(2 + (r.next_u64() % 31) as u32, 0, overflow, rounding);
        let to = Fixed::new(to.bits(), (r.next_u64() % u64::from(to.bits())) as u32, overflow, rounding);
        let moved = if to.fraction() >= fraction {
            i128::from(a) << (to.fraction() - fraction)
        } else {
            reference_shift(rounding, i128::from(a), fraction - to.fraction())
        };
        assert_eq!(f.convert(a, &to), reference_fit(to.bits(), overflow, moved) as i32);
    }
}

#[test]
fn conversion_errors_stay_within_a_step() {
    let mut r = SplitMix64::new(3);
    for bits in [8, 12, 16, 24] {
        for rounding in [Rounding::Truncate, Rounding::Nearest] {
            let f = Fixed::new(bits, bits - 1, Overflow::Saturate, rounding);
            for _ in 0..2000 {
                let x = 1.8 * r.next_centred();
                let e = f.value(f.code(x)) - x;
                let clipped = x >= 1.0 - f.lsb() || x < -1.0;
                if clipped {
                    continue;
                }
                match rounding {
                    Rounding::Truncate => assert!((-f.lsb()..=0.0).contains(&e), "{bits} {x} {e}"),
                    Rounding::Nearest => assert!(e.abs() <= 0.5 * f.lsb(), "{bits} {x} {e}"),
                }
            }
            assert_eq!(f.code(f64::NAN), 0);
            assert_eq!(f.code(5.0), f.range().1 as i32);
        }
    }
    // Wrap: one past the top is the bottom.
    let w = Fixed::new(12, 11, Overflow::Wrap, Rounding::Truncate);
    assert_eq!(w.fit(2048), -2048);
    assert_eq!(w.code(1.0), -2048);
}
