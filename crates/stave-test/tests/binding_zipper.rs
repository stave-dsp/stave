//! The continuous-binding check: a param
//! whose smoothing is DECLARED on the table (`with_smooth`) meets the
//! −60 dBc zipper instrument under block-stepped host automation —
//! the Step-1 harness re-aimed at the binding layer (`ParamRamp`),
//! not at a hand-rolled engine smoother.
//!
//! Two programs:
//!  - the Step-1 glide (0.25 → 1.0 exponential trajectory), targets
//!    sampled at block rate: the absolute −60 dBc limit. Per-sample vs
//!    64-sample stepping measured −131 vs −94 dBc in calibration;
//!    the binding must stay in that family.
//!  - a hostile full-swing square lane: a SEPARATION check. The
//!    glide's own bandwidth is physics (calibration note), so the
//!    assertion is relative — smoothing must buy ≥ 25 dB over raw
//!    steps — plus the instrument-teeth floor on the raw side.

use stave_core::params::{AnyParam, FloatParam, ParamId, Range, Smooth, Smoother};
use stave_core::{Environment, ParamBlock};
use stave_measure::zipper::{zipper_report, ZipperConfig};

const N: usize = 32_768;
const FS: f64 = 48_000.0;
const M: usize = 683; // ~1 kHz, bin-centred
const GUARD_HZ: f64 = 200.0;
const BLOCK: usize = 64;

/// Run a tone through the param's declared-smoothing read path,
/// with the target stepped at block rate by `automation(i)`.
fn run(p: &FloatParam, automation: impl Fn(usize) -> f64) -> Vec<f64> {
    let mut control = Smoother::new(p);
    control.prepare(&Environment::new(FS as f32, BLOCK));
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut out = vec![0.0f64; N];
    for b in 0..(N / BLOCK) {
        let i0 = b * BLOCK;
        p.set_value(automation(i0));
        let mut ramp = control.ramp_over(ParamBlock::empty(), p);
        for (k, slot) in out[i0..i0 + BLOCK].iter_mut().enumerate() {
            let i = i0 + k;
            *slot = ramp.next_f64() * (two_pi * M as f64 * i as f64 / N as f64).sin();
        }
    }
    out
}

fn square_lane(i: usize) -> f64 {
    if (i / 4096) % 2 == 0 {
        0.25
    } else {
        1.0
    }
}

/// The Step-1 calibration trajectory (zipper_conformance.rs), block-
/// sampled as a host lane would deliver it.
fn glide_lane(i: usize) -> f64 {
    let tau_samples = 0.015 * FS;
    0.25 + 0.75 * (1.0 - (-(i as f64) / tau_samples).exp())
}

#[test]
fn declared_smoothing_meets_the_zipper_limit() {
    let p = FloatParam::new(ParamId(1), "gain", Range::linear(0.0, 1.0)).default(0.25)
        .smooth(Smooth::ms(15.0));
    let r = zipper_report(
        &run(&p, glide_lane),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    assert!(
        r.spray_db < -60.0,
        "block-stepped glide through the binding sprays {:.1} dBc (limit −60)",
        r.spray_db
    );
}

#[test]
fn smoothing_separates_from_raw_steps() {
    let smoothed = FloatParam::new(ParamId(1), "gain", Range::linear(0.0, 1.0)).default(0.25)
        .smooth(Smooth::ms(15.0));
    let raw = FloatParam::new(ParamId(2), "gain", Range::linear(0.0, 1.0)).default(0.25)
        .smooth(Smooth::Off);
    let r_s = zipper_report(
        &run(&smoothed, square_lane),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    let r_r = zipper_report(
        &run(&raw, square_lane),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: 0.0,
        },
    );
    assert!(
        r_r.spray_db > -50.0,
        "unsmoothed square steps read {:.1} dBc — instrument lost its teeth",
        r_r.spray_db
    );
    assert!(
        r_s.spray_db < r_r.spray_db - 25.0,
        "declared smoothing buys only {:.1} dB over raw steps ({:.1} vs {:.1} dBc)",
        r_r.spray_db - r_s.spray_db,
        r_s.spray_db,
        r_r.spray_db
    );
}
