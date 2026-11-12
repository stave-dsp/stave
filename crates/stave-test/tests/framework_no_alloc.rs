//! FRAMEWORK no-alloc checks: the primitives themselves under the
//! guard. Project process-loop checks are COLOCATED (plugins/*/tests
//! /no_alloc.rs) on the shared stave_test::no_alloc harness.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::{assert_no_alloc, AllocDisabler};
use stave_dsp::RcClipperConfig;

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

/// Canary: the guard must actually fire — a check that cannot fail is
/// not a check. The AllocDisabler ABORTS (SIGABRT) rather than
/// unwinding, so this cannot be #[should_panic]; run it manually:
/// `cargo test -p stave-test --test no_alloc -- --ignored` and expect
/// the process to abort with "memory allocation of 3 bytes failed"
/// (verified 2026-07-04 — it does).
#[test]
#[ignore = "aborts by design; run manually to verify the guard"]
fn the_guard_itself_works() {
    assert_no_alloc(|| {
        let v = std::hint::black_box(vec![1u8, 2, 3]);
        assert_eq!(v.len(), 3);
    });
}

/// Framework-level generalization of the forge structural check:
/// the swap primitive ITSELF, hammered directly under the guard —
/// engage in both directions every iteration, zero allocations. Any
/// future project using SwapPoint inherits this proof.
#[test]
fn swap_point_engage_never_allocates() {
    use stave_dsp::{Chain, CurveStage, NonlinearRc, StageKind, SwapPoint};

    // Build OUTSIDE the guard (tables allocate at construction).
    let mut ch = Chain::<2>::new();
    let id = ch.push(StageKind::Curve(CurveStage::from_fn(
        -12.0,
        12.0,
        0.0,
        |x| x.tanh(),
    )));
    use stave_dsp::Stage as _;
    ch.prepare(192_000.0);
    let mut sp = SwapPoint::parking(StageKind::Rc(NonlinearRc::ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 3.0,
            threshold: 0.5,
            hardness: 4.0,
        },
        1.2,
    )));
    assert_no_alloc(|| {
        for i in 0..10_000 {
            let want: fn(&StageKind) -> bool = if i % 2 == 0 {
                |s| matches!(s, StageKind::Rc(_))
            } else {
                |s| matches!(s, StageKind::Curve(_))
            };
            assert!(sp.engage(&mut ch, id, want, 192_000.0));
            let _ = ch.tick(0.5);
        }
    });
}
