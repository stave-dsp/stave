//! Kani proof harnesses for the units (`cargo kani -p stave-dsp`,
//! scripts/kani.sh). The transcendental conversions (`powf`, `log10`,
//! `exp2`) are beyond the model checker, and symbolic `f64` division
//! (Ohm's law) does not finish in a reasonable time; the property tests
//! in `tests.rs` cover them. What is proved here holds for EVERY input.

use super::text::{prefix_index, SCALES};
use super::*;

/// The engineering-notation prefix never indexes out of the table, for
/// every `f64` (NaN, zero, the infinities, subnormals included), and
/// picks the largest scale at or below an in-range magnitude.
#[kani::proof]
#[kani::unwind(10)]
fn prefix_index_is_in_bounds_and_brackets() {
    let m: f64 = kani::any();
    let i = prefix_index(m);
    assert!(i < SCALES.len());
    if m >= SCALES[0] && m.is_finite() {
        assert!(SCALES[i] <= m);
        if i + 1 < SCALES.len() {
            assert!(m < SCALES[i + 1]);
        }
    }
}

/// Rounding a sample count to frames never panics, for every `f64`:
/// NaN and negative counts are zero frames, and whole counts are exact.
#[kani::proof]
fn frames_never_panic_and_floor_at_zero() {
    let v: f64 = kani::any();
    let n = Samples(v).frames();
    if v.is_nan() || v < 0.0 {
        assert_eq!(n, 0);
    }
    let whole: u16 = kani::any();
    assert_eq!(Samples::of_frames(whole as usize).frames(), whole as usize);
}
