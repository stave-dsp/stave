//! Devices with no SPICE counterpart, against independent high-accuracy
//! solutions of their defining equations (`stave-test/reference/*.txt`,
//! written by scripts/ja-reference.py), each a
//! `stave_test::reference::Case` with its metric and budget.

use stave_dsp::{JilesAtherton, JilesAthertonCore};
use stave_test::reference::{assert_converges, Case, Drive, Metric, Stimulus};

const STEEL: JilesAtherton = JilesAtherton {
    saturation: 1.6e6,
    shape: 1_100.0,
    coupling: 1.6e-3,
    pinning: 400.0,
    reversibility: 0.2,
};

/// Jiles & Atherton's 1986 steel driven at 5 kA/m, 50 Hz: the core's
/// trapezoidal-in-H discretization against SciPy's DOP853 (rtol 1e-12) on
/// the same ODE. The error must fall as h² — converging to the reference.
#[test]
fn jiles_atherton_matches_reference() {
    let case = Case::generated("jiles_atherton", "scripts/ja-reference.py")
        .stimulus(Stimulus::sine(5_000.0, 50.0))
        .budget(Metric::NormalizedRms, 5e-5);
    let at = |oversample: usize| {
        let mut core = JilesAthertonCore::new(STEEL, Default::default());
        let outcome = case.stage(Drive::oversampled(oversample), |h| core.step(h));
        assert_eq!(core.stats().capped, 0);
        outcome
    };
    let (coarse, fine) = (at(1), at(2));
    assert_converges(&coarse, &fine, 3.0);
    coarse.assert();
}

/// A transformer on that core (2000 turns, 6 cm², 10 cm path, 1 kΩ
/// source) driven at 400 V, 50 Hz into saturation, against SciPy's
/// DOP853 on the circuit's ODE. Converges as h², slightly under 4×: the
/// trapezoid crosses a turning-point kink.
#[test]
fn transformer_matches_reference() {
    use stave_dsp::{Stage, Transformer, TransformerConfig};
    let config = TransformerConfig {
        core: STEEL,
        primary_turns: 2_000.0,
        core_area: 6e-4,
        path_length: 0.1,
        turns_ratio: 1.0,
        source_ohms: 1_000.0,
        newton: Default::default(),
    };
    let case = Case::generated("transformer", "scripts/ja-reference.py")
        .stimulus(Stimulus::sine(400.0, 50.0))
        .budget(Metric::NormalizedRms, 1e-5);
    let at = |oversample: usize| {
        let mut t = Transformer::new(config);
        t.prepare(48_000.0 * oversample as f32);
        let outcome = case.stage(Drive::oversampled(oversample), |v| t.tick_f64(v));
        assert_eq!(t.stats().capped, 0);
        outcome
    };
    let (coarse, fine) = (at(1), at(2));
    assert_converges(&coarse, &fine, 2.5);
    coarse.assert();
}
