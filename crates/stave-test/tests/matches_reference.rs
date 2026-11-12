//! Devices with no SPICE counterpart, against independent high-accuracy
//! solutions of their defining equations. Regenerate with
//! scripts/ja-reference.py.

use std::path::PathBuf;

use stave_dsp::{JilesAtherton, JilesAthertonCore};
use stave_measure::waveform::Waveform;

fn reference(name: &str) -> Waveform {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("reference/{name}.txt"));
    Waveform::read(&path).unwrap()
}

/// Jiles & Atherton's 1986 steel driven at 5 kA/m, 50 Hz: the core's
/// trapezoidal-in-H discretization against SciPy's DOP853 (rtol 1e-12) on
/// the same ODE. The error must fall as h² — converging to the reference.
#[test]
fn the_jiles_atherton_core_matches_the_reference() {
    let r = reference("jiles_atherton");
    let model = JilesAtherton {
        saturation: 1.6e6,
        shape: 1_100.0,
        coupling: 1.6e-3,
        pinning: 400.0,
        reversibility: 0.2,
    };
    let deviation = |oversample: usize| {
        let rate = 48_000.0 * oversample as f64;
        let mut core = JilesAthertonCore::new(model, Default::default());
        let render: Vec<f64> = (0..(r.value.len() - 1) * oversample + 1)
            .map(|n| core.step(5_000.0 * (std::f64::consts::TAU * 50.0 * n as f64 / rate).sin()))
            .step_by(oversample)
            .collect();
        assert_eq!(core.stats().capped, 0);
        let d = r.difference(&render);
        d.normalized_rms
    };
    let (coarse, fine) = (deviation(1), deviation(2));
    assert!(coarse < 1e-4, "48 kHz: {coarse}");
    assert!(fine < coarse / 3.0, "not converging: {coarse} → {fine}");
}

/// A transformer on that core (2000 turns, 6 cm², 10 cm path, 1 kΩ
/// source) driven at 400 V, 50 Hz into saturation, against SciPy's
/// DOP853 on the circuit's ODE. Converges as h².
#[test]
fn the_transformer_matches_the_reference() {
    use stave_dsp::{Stage, Transformer, TransformerConfig};
    let r = reference("transformer");
    let config = TransformerConfig {
        core: JilesAtherton {
            saturation: 1.6e6,
            shape: 1_100.0,
            coupling: 1.6e-3,
            pinning: 400.0,
            reversibility: 0.2,
        },
        primary_turns: 2_000.0,
        core_area: 6e-4,
        path_length: 0.1,
        turns_ratio: 1.0,
        source_ohms: 1_000.0,
        newton: Default::default(),
    };
    let deviation = |oversample: usize| {
        let rate = 48_000.0 * oversample as f64;
        let mut t = Transformer::new(config);
        t.prepare(rate as f32);
        let render: Vec<f64> = (0..(r.value.len() - 1) * oversample + 1)
            .map(|n| {
                t.process_volts(400.0 * (std::f64::consts::TAU * 50.0 * n as f64 / rate).sin())
            })
            .step_by(oversample)
            .collect();
        assert_eq!(t.stats().capped, 0);
        let d = r.difference(&render);
        d.normalized_rms
    };
    let (coarse, fine) = (deviation(1), deviation(2));
    assert!(coarse < 5e-5, "48 kHz: {coarse}");
    // Slightly under 4×: the trapezoid crosses a turning-point kink.
    assert!(fine < coarse / 2.5, "not converging: {coarse} → {fine}");
}
