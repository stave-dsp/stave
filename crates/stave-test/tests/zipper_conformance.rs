//! Zipper-instrument calibration: the instrument IS the test suite
//! (house rule — synthetic signals with known answers before any
//! engine gets measured).
//!
//! Measured 2026-07-04 (N=32768, fs=48k, ~1 kHz carrier, guard 200 Hz):
//!   clean sine              −256 dBc   (numerical floor)
//!   exp ramp, per-sample    −131 dBc   (guard-band-confined)
//!   exp ramp, 64-spl steps   −94 dBc   (37 dB separation)
//!   100 ms linear, smooth    −94 dBc   (ramp skirts pass the guard —
//!                                       fast sweeps carry real
//!                                       bandwidth; that is physics,
//!                                       not zipper)
//!   100 ms linear, stepped   −76 dBc   (18 dB separation)
//! Limits below pin these with margin; a change that moves them is a
//! change to the instrument, not noise.

use stave_measure::zipper::{zipper_report, ZipperConfig};

const N: usize = 32_768;
const FS: f64 = 48_000.0;
const M: usize = 683; // ~1 kHz, bin-centred
const GUARD_HZ: f64 = 200.0;

fn tone_with_gain(gain: impl Fn(usize) -> f64) -> Vec<f64> {
    let two_pi = 2.0 * std::f64::consts::PI;
    (0..N)
        .map(|i| gain(i) * (two_pi * M as f64 * i as f64 / N as f64).sin())
        .collect()
}

/// Exponential glide sampled per SAMPLE — the smooth reference.
fn smooth_gain(i: usize) -> f64 {
    let tau_samples = 0.015 * FS;
    0.25 + 0.75 * (1.0 - (-(i as f64) / tau_samples).exp())
}

#[test]
fn constant_gain_reads_the_floor() {
    let r = zipper_report(
        &tone_with_gain(|_| 1.0),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    assert!(
        r.spray_db < -150.0,
        "clean sine spray {:.1} dBc (measured −256)",
        r.spray_db
    );
}

#[test]
fn smooth_ramp_stays_inside_the_guard() {
    let r = zipper_report(
        &tone_with_gain(smooth_gain),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    assert!(
        r.spray_db < -120.0,
        "per-sample exp ramp spray {:.1} dBc (measured −131)",
        r.spray_db
    );
}

#[test]
fn instrument_separates_stepped_from_smooth() {
    let stepped = tone_with_gain(|i| smooth_gain(i - i % 64));
    let r_step = zipper_report(
        &stepped,
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    let r_smooth = zipper_report(
        &tone_with_gain(smooth_gain),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    // Sensitivity: the instrument must SEE 64-sample steps of this
    // trajectory (measured −94.1 dBc)…
    assert!(
        r_step.spray_db > -105.0,
        "stepped trajectory reads {:.1} dBc — instrument lost sensitivity",
        r_step.spray_db
    );
    // …and separate them decisively from the per-sample glide
    // (measured separation 37 dB).
    assert!(
        r_step.spray_db > r_smooth.spray_db + 25.0,
        "separation collapsed: stepped {:.1} vs smooth {:.1} dBc",
        r_step.spray_db,
        r_smooth.spray_db
    );
}

#[test]
fn fast_linear_sweep_still_separates() {
    // 100 ms full-range sweep — automation at its most aggressive.
    let lin = |i: usize| (0.25 + 0.75 * (i as f64 / 4800.0)).min(1.0);
    let r_smooth = zipper_report(
        &tone_with_gain(lin),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    let r_step = zipper_report(
        &tone_with_gain(|i| lin(i - i % 64)),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    // Measured: smooth −94 (real sweep bandwidth passes the guard —
    // physics, not zipper), stepped −76.
    assert!(
        r_step.spray_db > r_smooth.spray_db + 12.0,
        "fast-sweep separation: stepped {:.1} vs smooth {:.1} dBc",
        r_step.spray_db,
        r_smooth.spray_db
    );
}
