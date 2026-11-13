//! The Korg35 against its analytic small-signal response, its
//! self-oscillation, its drive's harmonics, its rate invariance and its
//! stability under audio-rate modulation (aliasing: `tests/aliasing_budgets.rs`).

use super::Korg35;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::probe::{
    self, bounded_under_audio_rate_modulation, cutoff_sweep, div, poly, rate_invariant,
    self_oscillates, C,
};
use crate::units::hz;

/// `1 / (s² + (2 − 2r)·s + 1)`.
fn prototype(w: f64, r: f64) -> C {
    div((1.0, 0.0), poly(&[1.0, 2.0 - 2.0 * r, 1.0], (0.0, w)))
}

#[test]
fn cutoff_is_exact_across_the_range() {
    cutoff_sweep("korg35", |fc, r| Korg35::new(hz(fc as f32), r), prototype);
}

#[test]
fn self_oscillates_at_its_cutoff() {
    self_oscillates("korg35", |r| Korg35::new(hz(1_000.0), r), 0.005);
}

#[test]
fn drive_follows_the_cubic_law() {
    probe::drive_follows_the_cubic_law("korg35", || Korg35::new(hz(1_000.0), 0.5));
}

#[test]
fn is_rate_invariant() {
    rate_invariant(
        "korg35",
        |r| Korg35::new(hz(1_000.0), r),
        |w| prototype(w, 0.5),
    );
}

#[test]
fn tail_follows_the_linear_poles() {
    let mut f = Korg35::new(hz(1_000.0), 0.5);
    f.prepare(48_000.0);
    let Tail::Samples(n) = f.tail() else { panic!() };
    f.tick(0.01);
    let last = (1..4 * n)
        .filter(|_| f.tick(0.0).abs() > 1e-8)
        .last()
        .unwrap_or(0);
    assert!(last <= n, "rang {last} past {n}");
    f.set_resonance(1.0);
    assert_eq!(f.tail(), Tail::Infinite);
}

#[test]
fn stays_bounded_under_audio_rate_modulation() {
    bounded_under_audio_rate_modulation("korg35", |r| Korg35::new(hz(1_000.0), r));
}
