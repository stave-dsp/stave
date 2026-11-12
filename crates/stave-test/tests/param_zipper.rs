//! The params system's DEFAULT
//! smoothing suppresses zipper below −60 dBc under block-rate host
//! automation, and Smooth::Off on the same trajectory is measured
//! worse — the harness proves smoothing earns its cost, in both
//! directions.

use stave_core::params::{AnyParam, FloatParam, ParamId, Range, Smooth, Smoother};
use stave_core::{Environment, ParamBlock};
use stave_measure::zipper::{zipper_report, ZipperConfig};

const N: usize = 32_768;
const FS: f64 = 48_000.0;
const M: usize = 683; // ~1 kHz, bin-centred
const BLOCK: usize = 64;
const GUARD_HZ: f64 = 200.0;

/// Render a sine through a FloatParam-controlled gain, with the host
/// delivering block-rate target updates along an exp trajectory —
/// exactly what DAW automation playback does.
fn render(param: &FloatParam) -> Vec<f64> {
    let any = param as &dyn AnyParam;
    let mut control = Smoother::new(param);
    control.prepare(&Environment::new(FS as f32, BLOCK));
    let two_pi = 2.0 * std::f64::consts::PI;
    let tau_samples = 0.015 * FS;
    let trajectory = |i: usize| 0.25 + 0.75 * (1.0 - (-(i as f64) / tau_samples).exp());
    let mut out = Vec::with_capacity(N);
    for block_start in (0..N).step_by(BLOCK) {
        any.set_value(trajectory(block_start)); // host automation point
        let mut ramp = control.ramp_over(ParamBlock::empty(), param);
        for i in block_start..block_start + BLOCK {
            let g = ramp.next_f64();
            out.push(g * (two_pi * M as f64 * i as f64 / N as f64).sin());
        }
    }
    out
}

#[test]
fn default_smoothing_meets_the_limit_and_off_is_worse() {
    let smoothed = FloatParam::new(ParamId(1), "gain", Range::linear(0.0, 1.0)).default(0.25); // Smooth::DEFAULT
    let unsmoothed = FloatParam::new(ParamId(2), "gain", Range::linear(0.0, 1.0)).default(0.25)
        .smooth(Smooth::Off);

    let r_smooth = zipper_report(
        &render(&smoothed),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    let r_off = zipper_report(
        &render(&unsmoothed),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );

    // The limit: default smoothing under block-rate automation.
    assert!(
        r_smooth.spray_db < -60.0,
        "default smoothing spray {:.1} dBc (limit: < -60); worst {:.1} dB @ {:.0} Hz",
        r_smooth.spray_db,
        r_smooth.worst.1,
        r_smooth.worst.0
    );
    // Instrument sanity, the other direction: Off must read worse.
    assert!(
        r_off.spray_db > r_smooth.spray_db + 20.0,
        "Smooth::Off ({:.1} dBc) should exceed smoothed ({:.1} dBc) by > 20 dB",
        r_off.spray_db,
        r_smooth.spray_db
    );
}
