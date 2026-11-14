//! The rate contract as LAW.
//!
//! The convention: every stage derives its coefficients from the
//! rate it is prepared at, so a host re-prepare at a new rate re-derives
//! everything. These limits pin it:
//!  1. A re-prepared stage is indistinguishable from one built fresh at
//!     the new rate (no stale coefficient survives re-prepare).
//!  2. The response is rate-CORRECT: the −3 dB point stays at the
//!     spec's Hz at both rates. A stale-coefficient bug would leave the
//!     normalized cutoff fixed and the Hz cutoff would double.

use stave_dsp::{Biquad, FilterSpec, Stage};
use stave_measure::tone_window::ToneWindow;
use stave_dsp::units::Hz;

const N: usize = 8192;
const CUT_HZ: f32 = 1_000.0;

/// Response at the bin nearest `hz`, preparing nothing — the stage
/// arrives in whatever prepared state the test put it in.
fn resp_at(stage: &mut Biquad, fs: f64, hz: f64) -> f64 {
    let m = (hz * N as f64 / fs).round() as usize;
    stage.reset();
    ToneWindow {
        bin: m,
        window: N,
        sample_rate: fs,
        amplitude: 0.25,
        warmup_windows: 1,
    }
    .gain_db(|x: f32| stage.tick(x))
}

fn lowpass() -> Biquad {
    Biquad::new(FilterSpec::Lowpass {
        frequency: Hz::new(CUT_HZ),
        q: core::f32::consts::FRAC_1_SQRT_2,
    })
}

#[test]
fn reprepare_equals_fresh_prepare() {
    // Prepared at 48 k, run, then re-prepared at 96 k...
    let mut reprepared = lowpass();
    reprepared.prepare(48_000.0);
    let _ = resp_at(&mut reprepared, 48_000.0, 1_000.0);
    reprepared.prepare(96_000.0);

    // ...must run bit-identically to one born at 96 k.
    let mut fresh = lowpass();
    fresh.prepare(96_000.0);

    for probe_hz in [250.0, 1_000.0, 4_000.0] {
        let a = resp_at(&mut reprepared, 96_000.0, probe_hz);
        let b = resp_at(&mut fresh, 96_000.0, probe_hz);
        assert_eq!(a, b, "stale state survived re-prepare at {probe_hz} Hz");
    }
}

#[test]
fn response_is_rate_correct_at_both_rates() {
    let mut stage = lowpass();
    for fs in [48_000.0_f64, 96_000.0] {
        stage.prepare(fs as f32);
        let at_cut = resp_at(&mut stage, fs, CUT_HZ as f64);
        assert!(
            (at_cut + 3.01).abs() < 0.3,
            "cutoff not at {CUT_HZ} Hz for fs={fs}: {at_cut:.2} dB (want ≈ −3.01)"
        );
        let octave_up = resp_at(&mut stage, fs, 2.0 * CUT_HZ as f64);
        assert!(
            octave_up < -10.0,
            "skirt missing an octave above cutoff for fs={fs}: {octave_up:.2} dB"
        );
    }
    // The stale-coefficient failure shape, pinned from the other side:
    // 48 k coefficients run at 96 k would put the −3 dB point at 2 kHz.
    stage.prepare(48_000.0);
    let stale_shape = resp_at(&mut stage, 96_000.0, 2.0 * CUT_HZ as f64);
    assert!(
        (stale_shape + 3.01).abs() < 0.5,
        "sanity: mis-prepared run should show the doubled cutoff ({stale_shape:.2} dB)"
    );
}
