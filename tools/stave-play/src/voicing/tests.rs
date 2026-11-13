//! A whole session headlessly: prepare, trials, log, bless, verify.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;

use stave_host::config::{HostConfig, SampleRate};

use super::candidate::Candidate;
use super::log::{self, History, Meta};
use super::material::Material;
use super::takes::{self, Plan, Prepared};
use super::trials::{AbxListen, AbxRun, RatingRun, Stimulus};
use super::{bless, MaterialChoice};

const RATE: f64 = 48_000.0;

fn config() -> HostConfig {
    HostConfig {
        sample_rate: SampleRate::new(RATE).unwrap(),
        max_block: NonZeroUsize::new(512).unwrap(),
        ..HostConfig::default()
    }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("stave-voicing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn session(material: &str, specs: &[&str]) -> Prepared {
    let m = Material::builtin(material, RATE).unwrap();
    let candidates: Vec<Candidate> = specs.iter().map(|s| Candidate::parse(s).unwrap().0).collect();
    let jobs = takes::open_jobs(&candidates, &m, 0.0, config()).unwrap();
    let plan = Plan {
        material: m,
        trim_db: 0.0,
        candidates,
        reference: Some(0),
        with_anchor: true,
        block_size: 512,
    };
    takes::prepare(plan, jobs, &AtomicU32::new(0)).unwrap()
}

#[test]
fn a_session_log_records_everything_and_never_says_inaudible() {
    let p = session("guitar riff", &["dry", "stage:clipper/diode", "stage:filter/peak;db=4"]);
    let mut abx = AbxRun::new((0, 1), 16, 11);
    while !abx.finished() {
        let x = abx.take_for(AbxListen::X);
        abx.listen(AbxListen::X);
        // A listener right about two times in three.
        abx.answer(x == 0 || abx.trials.len() % 3 == 0);
        abx.confirm();
    }
    let stimuli = vec![
        Stimulus::Candidate { index: 1, take: 1 },
        Stimulus::Candidate { index: 2, take: 2 },
        Stimulus::HiddenReference { take: 0 },
        Stimulus::Anchor { take: p.anchor.unwrap() },
    ];
    let mut rating = RatingRun::new(stimuli, Some(0), 2, 5);
    while !rating.finished() {
        for slot in 0..rating.order.len() {
            rating.sliders[slot] = 40.0 + 10.0 * slot as f64;
        }
        rating.confirm();
    }
    let mut history = History::default();
    history.record_abx(&p, &abx);
    history.record_rating(&p, &rating);
    let meta = Meta {
        id: "test-session".into(),
        started: log::now_iso(),
        crossfade_ms: 10.0,
        loop_seconds: [0.0, p.material.seconds()],
    };
    let dir = temp("log");
    let built = log::build(&p, &meta, &history);
    let path = log::write(&built, &dir).unwrap();
    assert!(path.starts_with(log::sessions_dir(&dir)));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["format"], log::FORMAT);
    assert_eq!(json["material"]["pcm_sha256"], p.material.pcm_sha256.as_str());
    assert_eq!(json["candidates"].as_array().unwrap().len(), 3);
    assert_eq!(json["candidates"][0]["reference"], true);
    assert_eq!(json["abx"][0]["trials"].as_array().unwrap().len(), 16);
    assert!(json["abx"][0]["stats"]["power"].as_f64().unwrap() < 0.5, "16 trials are weak");
    assert!(json["abx"][0]["stats"]["d_prime"].is_number());
    assert_eq!(json["ratings"][0]["trials"].as_array().unwrap().len(), 2);
    assert!(json["loudness"]["max_deviation_lu"].as_f64().unwrap() <= takes::LOUDNESS_TOLERANCE_LU);
    let summary = std::fs::read_to_string(path.with_extension("txt")).unwrap();
    assert!(summary.contains("ABX dry vs stage:clipper/diode"), "{summary}");
    for line in summary.lines() {
        let l = line.to_lowercase();
        assert!(!l.contains("inaudible") || l.contains("not evidence"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_blessed_sound_replays_identically_and_compares_as_a_candidate() {
    let p = session("power chords", &["dry", "stage:clipper/diode", "stage:coupler"]);
    let dir = temp("bless");
    let record = bless::bless(&p, 1, "test-session", "the hot one", &dir).unwrap();
    // The record pins the settings, the material and the fingerprint
    // (DSP.md R18): the candidate's T0 characterisation as a black box.
    let (rec, audio) = bless::load(&record).unwrap();
    assert_eq!(rec.candidate.label, "stage:clipper/diode");
    assert_eq!(rec.material.pcm_sha256, p.material.pcm_sha256);
    let fp = rec.fingerprint.as_ref().expect("a stage candidate is fingerprinted");
    assert_eq!(fp["format"], bless::FINGERPRINT_FORMAT);
    assert!(fp["knee"].as_f64().unwrap() > 0.0);
    let text = fp["text"].as_str().unwrap();
    assert!(text.contains("metric 48000 tone/1k/+12 gedlee") && text.contains("plot harmonics"), "{text}");
    assert_eq!(audio.channels[0].len(), p.material.frames());
    // This build reproduces it bit for bit.
    let v = bless::verify(&record).unwrap();
    assert!(v.bit_identical, "residual {} dB", v.residual_db);
    assert_eq!(v.fingerprint_moved, Some(Vec::new()), "the fingerprint holds");
    // As a candidate on the same material it is the same sound as the
    // live stage: same loudness, same take.
    let blessed = Candidate::blessed(record.clone());
    let live = Candidate::parse("stage:clipper/diode").unwrap().0;
    let m = p.material.clone();
    let candidates = vec![live, blessed];
    let jobs = takes::open_jobs(&candidates, &m, 0.0, config()).unwrap();
    let plan = Plan {
        material: m,
        trim_db: 0.0,
        candidates,
        reference: None,
        with_anchor: false,
        block_size: 512,
    };
    let q = takes::prepare(plan, jobs, &AtomicU32::new(0)).unwrap();
    assert_eq!(q.takes[0].sha256, q.takes[1].sha256);
    // On other material, or at another trim, it refuses.
    let other = Material::builtin("keys", RATE).unwrap();
    let err = takes::open_jobs(&[Candidate::blessed(record.clone())], &other, 0.0, config()).err().unwrap();
    assert!(err.to_string().contains("different material"), "{err}");
    let err = takes::open_jobs(&[Candidate::blessed(record)], &p.material, 3.0, config()).err().unwrap();
    assert!(err.to_string().contains("trim"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_changed_model_shows_up_in_verification() {
    let p = session("guitar riff", &["dry", "stage:filter/peak;db=6"]);
    let dir = temp("drift");
    let record = bless::bless(&p, 1, "s", "", &dir).unwrap();
    // Simulate a later change: edit the blessed settings, as a model
    // change would alter the sound under the same settings.
    let text = std::fs::read_to_string(&record).unwrap();
    let mut rec: bless::BlessRecord = serde_json::from_str(&text).unwrap();
    rec.candidate.set("db", 6.5);
    std::fs::write(&record, serde_json::to_string(&rec).unwrap()).unwrap();
    let v = bless::verify(&record).unwrap();
    assert!(!v.bit_identical && v.residual_db > -60.0, "{}", v.residual_db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn setup_material_choices_load_at_the_session_rate() {
    let c = MaterialChoice::Builtin("drum loop".into());
    let m = c.load(44_100.0).unwrap();
    assert_eq!(m.sample_rate, 44_100.0);
}
