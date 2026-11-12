//! BandSaturator automation-zipper check. For a NONLINEAR
//! engine the naive "spray < −60 dBc" limit conflates two things: the
//! engine's own drive-dependent alias/IMD floor (static property of the
//! 4× stack on hot HF material) and what automation ADDS on top. The
//! honest zipper metric is the DELTA: sweep spray vs the static floor
//! at the drive the sweep reaches.
//!
//! Instrument notes: the carrier sits at ~11 kHz so every k≥2 harmonic
//! lands above the 20 kHz analysis window and only non-harmonic content
//! is counted. Analysis starts at 20 Hz: the tube bias point WALKS with
//! drive and passes the 5 Hz blocker as infrasonic energy (measured
//! −48 dBc at 4 Hz on the sweep) — circuit physics, not zipper.
//!
//! MEASURED 2026-07-04 (48 k, full-scale·0.5 11 kHz tone, 20 Hz–20 kHz):
//!   static floor, drive 12 dB:  tube −61.8 dBc,  tape −57.8 dBc
//!     (worst bins 16008 / 5008 Hz — stack IMD/alias products; they
//!      exist statically and grow with drive)
//!   drive sweep 1→12 dB:        tube −61.2,      tape −57.6
//!   mix sweep 0.3→1.0 (tube):   −71.4  = the 6 dB static floor exactly
//! Automation ADDS ≤ 0.6 dB over the drive-matched floor — the
//! glides do their job; there is no zipper to chase. (Also measured:
//! follow_shapers at 0.1 dB vs 0.25 dB granularity — identical spray;
//! unquantized drive_gain — identical; sweep with/without the 0 dB gate
//! crossing — identical.)

use stave_measure::zipper::{zipper_report, ZipperConfig};
use stave_multiband_comp_vst3::saturation::{BandSaturator, SaturationCharacter};

const N: usize = 32_768;
const FS: f64 = 48_000.0;
const M: usize = 7_509; // ≈ 11.0 kHz, bin-centred: 2·f0 = 22 kHz > analysis window
const BLOCK: usize = 64;
const GUARD_HZ: f64 = 200.0;
const LO_HZ: f64 = 20.0;

/// Render N samples of a bin-centred sine through the BandSaturator while
/// `automate` fires at every block boundary (what DAW playback does).
fn render(
    ty: SaturationCharacter,
    mut automate: impl FnMut(&mut BandSaturator, usize),
) -> Vec<f64> {
    let mut sat = BandSaturator::new();
    sat.prepare(FS as f32);
    sat.set_type(ty);
    sat.set_mix(1.0);
    sat.set_drive_db(6.0);
    automate(&mut sat, 0);
    let two_pi = 2.0 * std::f64::consts::PI;
    let tone = |i: usize| (0.5 * (two_pi * M as f64 * i as f64 / N as f64).sin()) as f32;
    // Settle: engagement, envelopes, glides from construction.
    for i in 0..8_192 {
        let _ = sat.process(tone(i));
    }
    let mut out = Vec::with_capacity(N);
    for i in 0..N {
        if i % BLOCK == 0 {
            automate(&mut sat, i);
        }
        out.push(f64::from(sat.process(tone(8_192 + i))));
    }
    out
}

fn spray(ty: SaturationCharacter, automate: impl FnMut(&mut BandSaturator, usize)) -> f64 {
    zipper_report(
        &render(ty, automate),
        &ZipperConfig {
            carrier_bin: M,
            sample_rate: FS,
            guard_hz: GUARD_HZ,
            low_cut_hz: LO_HZ,
        },
    )
    .spray_db
}

#[test]
fn automation_adds_nothing_over_the_drive_matched_floor() {
    let tau = 0.05 * FS;
    let drive_traj = move |i: usize| (1.0 + 11.0 * (1.0 - (-(i as f64) / tau).exp())) as f32;
    let mix_traj = move |i: usize| (0.3 + 0.7 * (1.0 - (-(i as f64) / tau).exp())) as f32;

    // Static floors, pinned (family checks: if the ENGINE floor moves,
    // that is its own finding — investigate before re-pinning).
    let tube_floor12 = spray(SaturationCharacter::Tube, |s, _| s.set_drive_db(12.0));
    let tape_floor12 = spray(SaturationCharacter::Tape, |s, _| s.set_drive_db(12.0));
    let tube_floor6 = spray(SaturationCharacter::Tube, |_, _| {});
    assert!(
        (-65.0..=-58.0).contains(&tube_floor12),
        "tube 12 dB static floor out of family: {tube_floor12:.1} dBc (pinned −61.8)"
    );
    assert!(
        (-61.0..=-54.0).contains(&tape_floor12),
        "tape 12 dB static floor out of family: {tape_floor12:.1} dBc (pinned −57.8)"
    );

    // The zipper limits: automation adds ≤ 1 dB over the matched floor.
    let tube_sweep = spray(SaturationCharacter::Tube, |s, i| {
        s.set_drive_db(drive_traj(i))
    });
    let tape_sweep = spray(SaturationCharacter::Tape, |s, i| {
        s.set_drive_db(drive_traj(i))
    });
    let mix_sweep = spray(SaturationCharacter::Tube, |s, i| s.set_mix(mix_traj(i)));
    println!(
        "floors@12: tube {tube_floor12:.1} tape {tape_floor12:.1} | sweeps: tube {tube_sweep:.1} tape {tape_sweep:.1} mix {mix_sweep:.1} (floor@6 {tube_floor6:.1})"
    );
    assert!(
        tube_sweep < tube_floor12 + 1.0,
        "tube drive automation adds spray: sweep {tube_sweep:.1} vs floor {tube_floor12:.1}"
    );
    assert!(
        tape_sweep < tape_floor12 + 1.0,
        "tape drive automation adds spray: sweep {tape_sweep:.1} vs floor {tape_floor12:.1}"
    );
    assert!(
        mix_sweep < tube_floor6 + 1.0,
        "mix automation adds spray: sweep {mix_sweep:.1} vs floor {tube_floor6:.1}"
    );
}
