use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU32;

use stave_host::instance::Layout;
use stave_host::note::Event;

use super::*;
use crate::voicing::candidate::Candidate;

const RATE: f64 = 48_000.0;

fn config() -> HostConfig {
    HostConfig {
        sample_rate: stave_host::config::SampleRate::new(RATE).unwrap(),
        max_block: NonZeroUsize::new(256).unwrap(),
        ..HostConfig::default()
    }
}

/// Delays its input by `actual` frames and reports `reported`.
struct Delay {
    actual: usize,
    reported: u32,
    gain: f32,
    line: Vec<Vec<f32>>,
    pos: usize,
}

impl Delay {
    fn new(actual: usize, reported: u32, gain: f32) -> Self {
        Self {
            actual,
            reported,
            gain,
            line: vec![vec![0.0; actual + 1]; 2],
            pos: 0,
        }
    }
}

impl Instance for Delay {
    fn layout(&self) -> Layout {
        Layout {
            inputs: 2,
            outputs: 2,
            sample_rate: RATE,
            max_block: 256,
        }
    }
    fn latency(&self) -> u32 {
        self.reported
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[Event]) {
        let len = self.actual + 1;
        for i in 0..outputs[0].len() {
            for c in 0..2 {
                self.line[c][self.pos] = inputs[c][i];
                let read = (self.pos + len - self.actual) % len;
                outputs[c][i] = self.line[c][read] * self.gain;
            }
            self.pos = (self.pos + 1) % len;
        }
    }
}

fn material(name: &str) -> Material {
    Material::builtin(name, RATE).unwrap()
}

#[test]
fn latency_is_compensated_sample_for_sample() {
    let m = material("guitar riff");
    let input = trimmed(&m, 0.0);
    let mut d = Delay::new(237, 237, 1.0);
    let out = run(&mut d, &input, RATE, 256, |_| {});
    assert_eq!(out[0].len(), m.frames());
    for (i, (a, b)) in out[0].iter().zip(&input[0]).enumerate() {
        assert_eq!(a, b, "frame {i}");
    }
    let (lag, r) = measure_offset(&input, &out[0]).unwrap();
    assert!(lag == 0 && r > 0.999, "{lag} {r}");
}

#[test]
fn a_misreported_latency_is_measured_and_flagged() {
    let m = material("drum loop");
    let input = trimmed(&m, 0.0);
    let mut d = Delay::new(100, 40, 1.0);
    let out = run(&mut d, &input, RATE, 256, |_| {});
    let (lag, r) = measure_offset(&input, &out[0]).unwrap();
    assert_eq!(lag, 60, "100 actual − 40 reported");
    assert!(r > 0.9, "{r}");
    // Through prepare, the take carries a warning.
    let candidates = vec![Candidate::dry(), Candidate::plugin("test:delay".into())];
    let jobs = vec![
        Job::Dry,
        Job::Instance {
            instance: Box::new(Delay::new(100, 40, 1.0)),
            resolved: Vec::new(),
        },
    ];
    let p = prepare(Plan { material: m, trim_db: 0.0, candidates, reference: None, with_anchor: false, block_size: 256 }, jobs, &AtomicU32::new(0)).unwrap();
    assert!(p.takes[1].warnings[0].contains("+60 frames"), "{:?}", p.takes[1].warnings);
    assert!(p.takes[0].warnings.is_empty());
}

#[test]
fn loudness_matches_within_a_tenth_of_a_lu_on_any_material() {
    for name in ["guitar riff", "drum loop", "band mix", "noise bursts", "keys"] {
        let m = material(name);
        let input = trimmed(&m, 0.0);
        let frames = m.frames();
        // Three very different candidates: dry, 14 dB hotter, a dull
        // version 9 dB quieter.
        let dry = to_stereo(&input, frames);
        let hot: Stereo = [dry[0].iter().map(|v| v * 5.0).collect(), dry[1].iter().map(|v| v * 5.0).collect()];
        let dull = low_pass(&dry, 1_500.0, RATE);
        let dull: Stereo = [dull[0].iter().map(|v| v * 0.35).collect(), dull[1].iter().map(|v| v * 0.35).collect()];
        let m = match_loudness(&[&dry, &hot, &dull], RATE).unwrap();
        assert!(m.max_deviation_lu <= REFINE_LU, "{name}: {}", m.max_deviation_lu);
        // Check independently: scale each take and meter it from scratch.
        for (i, t) in [&dry, &hot, &dull].into_iter().enumerate() {
            let g = m.gain(i);
            let l: Vec<f32> = t[0].iter().map(|v| v * g).collect();
            let r: Vec<f32> = t[1].iter().map(|v| v * g).collect();
            let got = Lufs::integrated().measure(&[&l, &r], RATE).unwrap();
            assert!((got - m.target_lufs).abs() <= LOUDNESS_TOLERANCE_LU, "{name} take {i}: {got} vs {}", m.target_lufs);
            let peak = l.iter().chain(&r).fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(20.0 * f64::from(peak).log10() <= PEAK_CEILING_DBFS + 1e-6, "{name} take {i} peaks at {peak}");
        }
        // The hot take needed the common turn-down for headroom.
        assert!(m.headroom_db <= 0.0);
    }
}

#[test]
fn a_silent_candidate_is_an_error_not_a_silent_comparison() {
    let m = material("keys");
    let candidates = vec![Candidate::dry(), Candidate::plugin("test:mute".into())];
    let jobs = vec![
        Job::Dry,
        Job::Instance {
            instance: Box::new(Delay::new(1, 1, 0.0)),
            resolved: Vec::new(),
        },
    ];
    let err = prepare(Plan { material: m, trim_db: 0.0, candidates, reference: None, with_anchor: false, block_size: 256 }, jobs, &AtomicU32::new(0)).err().unwrap();
    assert!(matches!(err, PrepareError::Silent { .. }), "{err}");
}

#[test]
fn a_catalog_stage_against_dry_with_an_anchor() {
    let m = material("band mix");
    let (peak, _) = Candidate::parse("stage:filter/peak;db=9;hz=2000").unwrap();
    let candidates = vec![Candidate::dry(), peak];
    let jobs = open_jobs(&candidates, &m, -6.0, config()).unwrap();
    let progress = AtomicU32::new(0);
    let p = prepare(Plan { material: m, trim_db: -6.0, candidates, reference: Some(0), with_anchor: true, block_size: 256 }, jobs, &progress).unwrap();
    assert_eq!(progress.load(Ordering::Relaxed), 1000);
    assert_eq!(p.takes.len(), 3);
    assert_eq!(p.anchor, Some(2));
    assert_eq!(p.takes[2].kind, TakeKind::Anchor { from: 0 });
    let resolved = &p.takes[1].resolved;
    assert!(resolved.iter().any(|r| r.name == "db" && r.value == 9.0), "{resolved:?}");
    // A linear stage keeps the timing: the offset check finds no lag.
    assert_eq!(p.takes[1].offset.map(|o| o.0), Some(0));
    assert!(p.matching.max_deviation_lu <= LOUDNESS_TOLERANCE_LU);
    // The peak boost is louder raw, so it was turned down the most.
    assert!(p.matching.gain_db[1] < p.matching.gain_db[0]);
    // Equal takes hash equal; different ones do not.
    assert_ne!(p.takes[0].sha256, p.takes[1].sha256);
}

#[test]
fn bad_settings_fail_before_anything_runs() {
    let m = material("keys");
    let (c, _) = Candidate::parse("stage:filter/peak;loudness=11").unwrap();
    let err = open_jobs(&[c], &m, 0.0, config()).err().unwrap();
    assert!(err.to_string().contains("loudness"), "{err}");
    let (c, _) = Candidate::parse("stage:nope").unwrap();
    assert!(matches!(open_jobs(&[c], &m, 0.0, config()), Err(PrepareError::Open { .. })));
}

#[test]
fn trim_sets_the_drive() {
    let m = material("keys");
    let a = trimmed(&m, -6.0);
    let ratio = a[0].iter().zip(&m.channels[0]).find(|(_, b)| b.abs() > 0.1).map(|(a, b)| a / b).unwrap();
    assert!((f64::from(ratio) - db_to_gain(-6.0)).abs() < 1e-6);
}
