//! Null tests on synthetic cases: the alignment recovers a known level
//! and latency, the verdict reads a known error, and the residual
//! played back is that error.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;

use stave_host::config::{HostConfig, SampleRate};

use super::*;
use crate::voicing::material::Material;

const RATE: f64 = 48_000.0;
const MATERIAL: &str = "guitar riff";

fn config() -> HostConfig {
    HostConfig {
        sample_rate: SampleRate::new(RATE).unwrap(),
        max_block: NonZeroUsize::new(512).unwrap(),
        ..HostConfig::default()
    }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("stave-null-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The material, changed by `f` (per channel), written as a capture.
fn capture(dir: &Path, name: &str, f: impl Fn(&[f32]) -> Vec<f32>) -> String {
    let m = Material::builtin(MATERIAL, RATE).unwrap();
    let path = dir.join(format!("{name}.wav"));
    let audio = stave_host::wav::Audio {
        sample_rate: RATE as u32,
        channels: m.channels.iter().map(|c| f(c)).collect(),
    };
    stave_host::wav::write(&path, &audio).unwrap();
    format!("wav:{}", path.display())
}

/// A session of two candidates, the second the reference, nulled.
fn session(a: &str, b: &str) -> (Prepared, NullTest) {
    let m = Material::builtin(MATERIAL, RATE).unwrap();
    let candidates: Vec<Candidate> = [a, b].iter().map(|s| Candidate::parse(s).unwrap().0).collect();
    let jobs = takes::open_jobs(&candidates, &m, 0.0, config()).unwrap();
    let plan = takes::Plan {
        material: m,
        trim_db: 0.0,
        candidates,
        reference: Some(1),
        with_anchor: false,
        block_size: 512,
    };
    let mut p = takes::prepare(plan, jobs, &AtomicU32::new(0)).unwrap();
    p.nulls = all(&p);
    let n = p.nulls.as_ref().unwrap()[0].clone();
    println!("{}", n.describe(RATE));
    (p, n)
}

fn tone(len: usize, hz: f64, amplitude: f64) -> Vec<f32> {
    (0..len).map(|i| (amplitude * (std::f64::consts::TAU * hz * i as f64 / RATE).sin()) as f32).collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

#[test]
fn identical_takes_sit_at_the_floor() {
    let (_, n) = session("dry", "dry");
    assert!(n.inaudible && n.nmr_total_db < -100.0, "{n:?}");
    assert!(n.gain_db.abs() < 1e-6 && n.delay_samples.abs() < 1e-6);
    assert_eq!((n.candidate, n.reference), (0, 1));
}

#[test]
fn a_late_quiet_capture_aligns_and_is_inaudible() {
    let dir = temp("late");
    let g = 10f32.powf(-4.0 / 20.0);
    let late = capture(&dir, "late", |c| {
        let mut v = vec![0.0f32; 37];
        v.extend(c.iter().map(|x| x * g));
        v.truncate(c.len());
        v
    });
    let (_, n) = session("dry", &late);
    // The dry take needs 4 dB less and 37 samples' delay to meet it.
    assert!((n.gain_db + 4.0).abs() < 1e-3, "{n:?}");
    assert!((n.delay_samples - 37.0).abs() < 1e-2, "{n:?}");
    assert!(n.inaudible, "{}", n.describe(RATE));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_added_tone_is_heard_and_is_the_residual() {
    let dir = temp("tone");
    let m = Material::builtin(MATERIAL, RATE).unwrap();
    let level = rms(&m.channels[0]);
    let with = |db: f64| {
        let a = level * 10f64.powf(db / 20.0) * std::f64::consts::SQRT_2;
        move |c: &[f32]| -> Vec<f32> { c.iter().zip(tone(c.len(), 1_000.0, a)).map(|(x, t)| x + t).collect() }
    };
    let loud = capture(&dir, "loud", with(-20.0));
    let (p, n) = session("dry", &loud);
    assert!(!n.inaudible && n.verdict() == "AUDIBLE", "{}", n.describe(RATE));
    assert!((n.nmr_max_hz - 1_000.0).abs() < 250.0, "{n:?}");
    assert!((n.residual_db + 20.0).abs() < 0.5, "the residual is the tone, 20 dB down: {n:?}");
    // The residual played back is the tone, as heard (the reference's
    // matching gain), away from the ends.
    let gb = takes::db_to_gain(p.matching.gain_db[1]);
    let t = tone(n.residual[0].len(), 1_000.0, level * 0.1 * std::f64::consts::SQRT_2);
    let (lo, hi) = (4_800, n.residual[0].len() - 4_800);
    let dot: f64 = n.residual[0][lo..hi].iter().zip(&t[lo..hi]).map(|(r, t)| f64::from(*r) * f64::from(*t)).sum();
    let r = dot / (rms(&n.residual[0][lo..hi]) * rms(&t[lo..hi]) * (hi - lo) as f64);
    assert!(r > 0.99, "residual correlates {r} with the tone");
    assert!((rms(&n.residual[0][lo..hi]) / (gb * rms(&t[lo..hi])) - 1.0).abs() < 0.02);
    // Two player takes per null: true level, then raised to the session.
    let extra = player_takes(p.nulls.as_ref().unwrap());
    assert_eq!(extra.len(), 2);
    assert_eq!(extra[0].1, 1.0);
    assert!(extra[1].1 > 1.0);
    // 100 dB under the material: inaudible.
    let faint = capture(&dir, "faint", with(-100.0));
    let (_, n) = session("dry", &faint);
    assert!(n.inaudible, "{}", n.describe(RATE));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn headless_runs_the_same_null_and_writes_the_residual() {
    let dir = temp("headless");
    let g = 10f32.powf(-1.5 / 20.0);
    let capture = capture(&dir, "quiet", |c| c.iter().map(|x| x * g).collect());
    let out = dir.join("residual.wav");
    let h = Headless {
        a: "dry".into(),
        b: capture,
        material: Some(MATERIAL.into()),
        trim_db: 0.0,
        residual: Some(out.clone()),
    };
    let (n, rate) = headless(&h, config()).unwrap();
    assert_eq!(rate, RATE);
    assert!(n.inaudible && (n.gain_db + 1.5).abs() < 1e-3, "{n:?}");
    let written = stave_host::wav::read(&out).unwrap();
    assert_eq!(written.channels.len(), 2);
    assert_eq!(written.channels[0].len(), n.residual[0].len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn below_the_ear_models_rate_there_is_no_verdict() {
    let x: Stereo = [vec![0.1; 1000], vec![0.1; 1000]];
    assert!(between(&x, &x, (1.0, 1.0), 22_050.0, -23.0).is_err());
}
