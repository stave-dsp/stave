//! The ladders against their analytic small-signal responses, their
//! self-oscillation, their drive's harmonics, their rate invariance and
//! their stability under audio-rate modulation (aliasing: `tests/aliasing_budgets.rs`).

use super::{Huovilainen, Stinchcombe};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::probe::{
    bounded_under_audio_rate_modulation, cutoff_sweep, div, drive_follows_the_cubic_law, poly,
    rate_invariant, self_oscillates, C,
};
use crate::units::hz;

/// `1 / ((1 + s)⁴ + 4r)`.
fn transistor(w: f64, r: f64) -> C {
    div(
        (1.0, 0.0),
        poly(&[1.0, 4.0, 6.0, 4.0, 1.0 + 4.0 * r], (0.0, w)),
    )
}

/// `2 / (s⁴ + 8s³ + 20s² + 16s + 2 + 34r)`, `s` in units of the pitch / √2.
fn diode(w: f64, r: f64) -> C {
    let s = (0.0, w * std::f64::consts::SQRT_2);
    div((2.0, 0.0), poly(&[1.0, 8.0, 20.0, 16.0, 2.0 + 34.0 * r], s))
}

#[test]
fn huovilainen_cutoff_is_exact_across_the_range() {
    cutoff_sweep(
        "huovilainen",
        |fc, r| Huovilainen::new(hz(fc as f32), r),
        transistor,
    );
}

#[test]
fn stinchcombe_cutoff_is_exact_across_the_range() {
    cutoff_sweep(
        "stinchcombe",
        |fc, r| Stinchcombe::new(hz(fc as f32), r),
        diode,
    );
}

#[test]
fn huovilainen_self_oscillates_at_its_cutoff() {
    self_oscillates("huovilainen", |r| Huovilainen::new(hz(1_000.0), r), 0.005);
}

#[test]
fn stinchcombe_self_oscillates_at_its_cutoff() {
    self_oscillates("stinchcombe", |r| Stinchcombe::new(hz(1_000.0), r), 0.005);
}

#[test]
fn huovilainen_drive_follows_the_cubic_law() {
    drive_follows_the_cubic_law("huovilainen", || Huovilainen::new(hz(1_000.0), 0.5));
}

#[test]
fn stinchcombe_drive_follows_the_cubic_law() {
    drive_follows_the_cubic_law("stinchcombe", || Stinchcombe::new(hz(1_000.0), 0.5));
}

#[test]
fn huovilainen_is_rate_invariant() {
    rate_invariant(
        "huovilainen",
        |r| Huovilainen::new(hz(1_000.0), r),
        |w| transistor(w, 0.5),
    );
}

#[test]
fn stinchcombe_is_rate_invariant() {
    rate_invariant(
        "stinchcombe",
        |r| Stinchcombe::new(hz(1_000.0), r),
        |w| diode(w, 0.5),
    );
}

#[test]
fn tails_follow_the_linear_poles() {
    let mut f = Huovilainen::new(hz(1_000.0), 0.5);
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
    let mut d = Stinchcombe::new(hz(1_000.0), 0.5);
    d.prepare(48_000.0);
    assert!(matches!(d.tail(), Tail::Samples(_)));
    d.set_resonance(1.2);
    assert_eq!(d.tail(), Tail::Infinite);
}

#[test]
fn ladders_stay_bounded_under_audio_rate_modulation() {
    bounded_under_audio_rate_modulation("huovilainen", |r| Huovilainen::new(hz(1_000.0), r));
    bounded_under_audio_rate_modulation("stinchcombe", |r| Stinchcombe::new(hz(1_000.0), r));
}
