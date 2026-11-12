//! Conformance: the Stage/Chain layer, judged through the G(f, level)
//! surface — the instrument that distinguishes "a curve" from "an amp".
//!
//! Ground truth used here: a MEMORYLESS nonlinearity's fundamental gain
//! at a given input level is identical at every frequency (its surface
//! columns match). Interstage filters, slew, and stateful stages bend the
//! surface — gain compression that depends on frequency and history is
//! exactly what tube EQs, amps, and compressors measure like.

use stave_dsp::RcClipperConfig;
use stave_dsp::{
    shapes, Biquad, Chain, CurveStage, FilterSpec, Frame, Hysteresis, NonlinearRc, Stage, StageKind,
};
use stave_measure::nonlinear::{GainSurface, HysteresisTest};

const FS: f64 = 48_000.0;

fn amp_chain() -> Chain<4> {
    let mut ch = Chain::<4>::new();
    let mut a = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(2.4));
    a.set_pending(Frame {
        bias: 0.1,
        drive: 3.0,
        gain: 1.0,
    });
    ch.push(StageKind::Curve(a)).unwrap();
    ch.push(StageKind::Filter(Biquad::new(FilterSpec::HighShelf {
        hz: 900.0,
        db: -9.0,
    })))
    .unwrap();
    let mut b = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(8.0));
    b.set_pending(Frame {
        bias: 0.0,
        drive: 1.6,
        gain: 1.0,
    });
    ch.push(StageKind::Curve(b)).unwrap();
    ch
}

/// The Chain claim, measured as a surface: interstage filtering makes
/// gain compression frequency-dependent (the amp bend); the same two
/// clippers without the filter produce a flat-across-frequency surface.
#[test]
fn chain_bends_the_gain_surface_and_memoryless_does_not() {
    let freqs = [200.0, 800.0, 2_000.0, 5_000.0];
    let levels = [-36.0, -24.0, -12.0, 0.0];
    let n = 8192;

    let surf = GainSurface {
        freqs_hz: freqs.to_vec(),
        levels_db: levels.to_vec(),
        window: n,
        sample_rate: FS,
        warmup_windows: 1,
    }
    .measure(|| {
        let mut ch = amp_chain();
        ch.prepare(FS as f32);
        move |x: f32| ch.tick(x)
    });
    let mut csv = Vec::new();
    surf.write_csv(&mut csv).unwrap();
    assert_eq!(
        csv.iter().filter(|&&b| b == b'\n').count(),
        surf.levels_db.len() + 1
    );
    let comp = surf.compression_db();
    let spread = comp.iter().cloned().fold(f64::MIN, f64::max)
        - comp.iter().cloned().fold(f64::MAX, f64::min);
    println!("chain compression per freq: {comp:?} (spread {spread:.2} dB)");

    let flat = GainSurface {
        freqs_hz: freqs.to_vec(),
        levels_db: levels.to_vec(),
        window: n,
        sample_rate: FS,
        warmup_windows: 1,
    }
    .measure(|| {
        // Same clippers, no interstage filter ⇒ memoryless composite.
        let mut cs = CurveStage::from_fn(-6.0, 6.0, 0.0, {
            let f = shapes::pre_gain(3.0, shapes::knee(2.4));
            let g = shapes::pre_gain(1.6, shapes::knee(8.0));
            shapes::compose(f, g)
        });
        cs.prepare(FS as f32);
        move |x: f32| cs.tick(x)
    });
    let fcomp = flat.compression_db();
    let fspread = fcomp.iter().cloned().fold(f64::MIN, f64::max)
        - fcomp.iter().cloned().fold(f64::MAX, f64::min);
    println!("memoryless compression per freq: {fcomp:?} (spread {fspread:.2} dB)");

    assert!(
        fspread < 0.4,
        "memoryless surface should be frequency-flat: spread {fspread:.2} dB"
    );
    assert!(
        spread > 2.0,
        "chain surface should bend with frequency: spread {spread:.2} dB"
    );
}

/// The implicit RC stage has real memory: its I/O loop opens (nonzero
/// area), unlike any memoryless curve — and the state biases the clip
/// history-dependently (verified in the unit tests; here the loop is the
/// instrument-level check).
#[test]
fn rc_clipper_and_hysteresis_open_io_loops() {
    let mut rc = NonlinearRc::diode_clipper(&RcClipperConfig {
        tau_ms: 3.0,
        threshold: 0.35,
        hardness: 6.0,
    });
    rc.prepare(FS as f32);
    let a_rc = HysteresisTest {
        frequency_hz: 200.0,
        sample_rate: FS,
        amplitude: 0.9,
    }
    .loop_area(|x: f32| rc.tick(x));
    println!("RC stage loop area @200 Hz: {a_rc:.4}");
    assert!(a_rc > 0.005, "RC stage shows no memory: {a_rc}");

    let mut hy = Hysteresis::new(1.5, 0.8);
    hy.prepare(FS as f32);
    let a_lo = HysteresisTest {
        frequency_hz: 50.0,
        sample_rate: FS,
        amplitude: 0.8,
    }
    .loop_area(|x: f32| hy.tick(x));
    let a_hi = HysteresisTest {
        frequency_hz: 500.0,
        sample_rate: FS,
        amplitude: 0.8,
    }
    .loop_area(|x: f32| hy.tick(x));
    println!("hysteresis loop area: 50 Hz {a_lo:.4} | 500 Hz {a_hi:.4}");
    assert!(a_lo > 0.01, "no hysteresis loop: {a_lo}");
    let ratio = (a_lo / a_hi).max(a_hi / a_lo);
    assert!(
        ratio < 2.5,
        "loop area should be ~frequency-independent: {a_lo} vs {a_hi}"
    );

    // Control: a memoryless CurveStage's loop area is ~zero — with ADAA
    // DISABLED. (Instructive instrument finding: first-order ADAA is a
    // rectangular-kernel average, i.e. an inherent HALF-SAMPLE lag, which
    // opens a tiny frequency-proportional ellipse — lag, not hysteresis.
    // Measured ~0.03 at 200 Hz with ADAA on.)
    let mut cs = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(4.0));
    cs.set_adaa(false);
    cs.prepare(FS as f32);
    let a_curve = HysteresisTest {
        frequency_hz: 200.0,
        sample_rate: FS,
        amplitude: 0.9,
    }
    .loop_area(|x: f32| cs.tick(x));
    println!("memoryless control loop area (ADAA off): {a_curve:.6}");
    assert!(
        a_curve < 1e-3,
        "memoryless stage should not loop: {a_curve}"
    );
}
