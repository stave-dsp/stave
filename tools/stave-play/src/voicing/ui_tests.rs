//! The Voicing view driven headlessly: real egui frames with simulated
//! keys, no window and no audio device, through setup, the modes, the
//! null test and the log.

use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;
use eframe::egui::{self, Event, Key, Modifiers};
use stave_host::config::{HostConfig, SampleRate};

use stave_host::instance::Instance;

use super::log::Choice;
use super::player::Player;
use super::trials::{AbxListen, AbxRun};
use super::{Ctx, Launch, Mode, Voicing};
use crate::analysis::AnalysisRing;
use crate::audio::{Shared, MAX_EVENTS};
use crate::debug_log::DebugLog;

fn shared() -> Shared {
    Shared {
        events: Arc::new(ArrayQueue::new(MAX_EVENTS)),
        samples: Arc::new(ArrayQueue::new(4)),
        peak: Arc::new(AtomicU32::new(0)),
        output_tap: AnalysisRing::new(),
        input_tap: AnalysisRing::new(),
        log: DebugLog::new(),
    }
}

fn key(k: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

struct Harness {
    ctx: egui::Context,
    shared: Shared,
    config: HostConfig,
    voicing: Voicing,
}

impl Harness {
    fn new(launch: Launch) -> Self {
        let mut voicing = Voicing::new(&launch).unwrap();
        voicing.start_audio = false;
        Self {
            ctx: egui::Context::default(),
            shared: shared(),
            config: HostConfig {
                sample_rate: SampleRate::new(48_000.0).unwrap(),
                max_block: NonZeroUsize::new(512).unwrap(),
                ..HostConfig::default()
            },
            voicing,
        }
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 900.0))),
            ..Default::default()
        };
        let mut suspend = || {};
        let mut ctx = Ctx {
            config: self.config,
            shared: &self.shared,
            main_plugin: None,
            suspend_main: &mut suspend,
        };
        let v = &mut self.voicing;
        let _ = self.ctx.run(input, |c| {
            egui::CentralPanel::default().show(c, |ui| v.ui(ui, &mut ctx));
        });
    }

    fn press(&mut self, k: Key) {
        self.frame(vec![key(k, Modifiers::NONE)]);
    }

    fn press_shift(&mut self, k: Key) {
        self.frame(vec![key(k, Modifiers::SHIFT)]);
    }

    fn until_live(&mut self) {
        let t0 = Instant::now();
        while self.voicing.live.is_none() {
            assert!(t0.elapsed() < Duration::from_secs(60), "never went live: {:?}", self.voicing.status);
            self.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn launch(dir: &std::path::Path) -> Launch {
    Launch {
        candidates: vec![
            "dry;ref".into(),
            "stage:filter/peak;db=6;label=peak".into(),
            "stage:clipper/diode".into(),
        ],
        material: Some("keys".into()),
        trim_db: Some(-3.0),
        abx_trials: Some(4),
        dir: Some(dir.into()),
    }
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("stave-voicing-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn a_whole_session_from_the_keyboard() {
    let dir = temp("session");
    let mut h = Harness::new(launch(&dir));
    h.until_live();
    {
        let l = h.voicing.live.as_ref().unwrap();
        assert_eq!(l.prepared.candidates.len(), 3);
        assert_eq!(l.prepared.reference, Some(0));
        assert!(l.prepared.anchor.is_some());
        assert_eq!(l.prepared.trim_db, -3.0);
        assert!(l.saved.is_some(), "a log from the start");
    }

    // The default screen: Dry is the given dry candidate (also the
    // reference), A and B the others; it starts on Dry; space plays,
    // D / digits / letters select.
    {
        let l = h.voicing.live.as_ref().unwrap();
        assert_eq!((l.dry, l.letters.clone()), (0, vec![1, 2]));
        assert_eq!(l.transport.selected(), 0, "starts on Dry");
        assert_eq!(l.title(), "peak vs stage:clipper/diode");
    }
    h.press(Key::Space);
    assert!(h.voicing.live.as_ref().unwrap().transport.is_playing());
    h.press(Key::Num2);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 2);
    h.press(Key::A);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 1);
    h.press(Key::D);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 0);
    h.press(Key::B);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 2);
    h.press(Key::Num9); // no ninth letter: ignored
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 2);
    h.press(Key::Num0);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 0);
    // The advanced modes' keys apply while Advanced is open.
    h.voicing.live.as_mut().unwrap().advanced_open = true;

    // Loop keys.
    h.press(Key::CloseBracket);
    h.press(Key::Backspace);
    let l = h.voicing.live.as_ref().unwrap();
    assert_eq!(l.transport.loop_region(), (0, l.transport.frames()));

    // ABX: four trials from the keyboard.
    let l = h.voicing.live.as_mut().unwrap();
    l.mode = Mode::Abx;
    l.abx = Some(AbxRun::new((0, 1), l.abx_trials, 42));
    for _ in 0..4 {
        h.press(Key::X);
        let x = {
            let l = h.voicing.live.as_ref().unwrap();
            l.transport.selected()
        };
        // Answer correctly: X plays take 0 → "X is A".
        if x == 0 {
            h.press_shift(Key::A);
        } else {
            h.press(Key::ArrowRight);
        }
        h.press(Key::Enter);
    }
    {
        let l = h.voicing.live.as_ref().unwrap();
        let run = l.abx.as_ref().unwrap();
        assert!(run.finished());
        assert_eq!(run.score().correct, 4);
        assert_eq!(l.history.abx.len(), 1);
        assert!(run.trials.iter().all(|t| t.listens[2] >= 1));
    }

    // Rating: digits play slots, arrows move the slider, enter confirms.
    let l = h.voicing.live.as_mut().unwrap();
    l.mode = Mode::Rating;
    let (stimuli, open) = l.rating_stimuli();
    assert_eq!(stimuli.len(), 4, "two candidates, hidden reference, anchor");
    l.rating = Some(super::trials::RatingRun::new(stimuli, open, 1, 7));
    h.press(Key::Num2);
    h.press_shift(Key::ArrowUp);
    h.press(Key::ArrowDown);
    {
        let l = h.voicing.live.as_ref().unwrap();
        let run = l.rating.as_ref().unwrap();
        assert_eq!(run.sliders[1], 59.0, "50 + 10 − 1");
        assert_eq!(l.transport.selected(), run.take_at(1).unwrap());
    }
    h.press(Key::R);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 0, "the open reference");
    h.press(Key::Enter);
    assert!(h.voicing.live.as_ref().unwrap().rating.as_ref().unwrap().finished());
    // Results are drawn without trouble.
    h.frame(Vec::new());

    // Null test: both candidates nulled against the dry reference, the
    // panel drawn, digits still select, the residual takes playable.
    let l = h.voicing.live.as_mut().unwrap();
    l.mode = Mode::Null;
    {
        let nulls = l.prepared.nulls.as_ref().unwrap();
        assert_eq!(nulls.len(), 2);
        assert!(nulls.iter().all(|n| n.reference == 0 && !n.inaudible), "a peak and a clipper are heard against dry");
    }
    h.frame(Vec::new());
    h.press(Key::Num2);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 1, "null test: digits by number");
    let l = h.voicing.live.as_mut().unwrap();
    let residual = l.residual_take(1, true);
    assert_eq!(residual, l.prepared.takes.len() + 3);
    l.select(residual);
    h.frame(Vec::new());

    // End: the log holds it all.
    let path = h.voicing.live.as_ref().unwrap().saved.as_ref().unwrap().0.clone();
    h.voicing.end_session();
    assert!(h.voicing.live.is_none());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["abx"][0]["stats"]["correct"], 4);
    assert_eq!(json["ratings"][0]["trials"].as_array().unwrap().len(), 1);
    assert_eq!(json["material"]["trim_db"], -3.0);
    assert_eq!(json["nulls"].as_array().unwrap().len(), 2);
    assert_eq!(json["nulls"][0]["b"], "dry");
    std::fs::remove_dir_all(dir).unwrap();
}

/// The listening session's shape: two variants, no dry given, `;ref`
/// on the second (so it is A).
fn variants(dir: &std::path::Path) -> Launch {
    Launch {
        candidates: vec![
            "stage:filter/peak;db=2;label=peak 2".into(),
            "stage:filter/peak;db=9;label=peak 9;ref".into(),
        ],
        material: Some("keys".into()),
        trim_db: None,
        abx_trials: Some(6),
        dir: Some(dir.into()),
    }
}

/// Pull `frames` from the player in blocks.
fn pull(p: &mut Player, frames: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    for (lc, rc) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        p.process(&[], &mut [lc, rc], &[]);
    }
    l
}

#[test]
fn dry_is_always_there_and_every_source_plays_in_place() {
    let dir = temp("sources");
    let mut h = Harness::new(variants(&dir));
    h.until_live();
    h.frame(Vec::new());
    let (dry, a, b) = {
        let l = h.voicing.live.as_ref().unwrap();
        assert_eq!(l.prepared.candidates.len(), 3, "Dry added");
        assert_eq!(l.dry, 2);
        assert_eq!(l.letters, vec![1, 0], ";ref is A");
        assert_eq!(l.title(), "peak 9 vs peak 2");
        assert_eq!(l.buttons(), vec![2, 1, 0]);
        // Three different sounds, not one.
        let shas: Vec<&str> = (0..3).map(|c| l.prepared.takes[l.take_of(c)].sha256.as_str()).collect();
        assert!(shas[0] != shas[1] && shas[1] != shas[2] && shas[0] != shas[2], "{shas:?}");
        (l.take_of(2), l.take_of(1), l.take_of(0))
    };
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), dry);
    for (k, want) in [(Key::Num1, a), (Key::Num2, b), (Key::D, dry), (Key::A, a), (Key::B, b), (Key::Num0, dry)] {
        h.press(k);
        assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), want, "{k:?}");
    }

    // Switching keeps the playhead: drive the real player from the
    // view's transport, switch from the keyboard between blocks.
    let (mut player, t, takes) = {
        let l = h.voicing.live.as_ref().unwrap();
        let takes: Vec<_> = l
            .prepared
            .takes
            .iter()
            .enumerate()
            .map(|(i, t)| (t.audio.clone(), l.prepared.matching.gain(i)))
            .collect();
        let p = Player::new(takes.clone(), l.transport.clone(), l.prepared.sample_rate, 512);
        (p, l.transport.clone(), takes)
    };
    t.set_playing(true);
    let mut at = 0usize;
    for (k, take) in [(Key::D, dry), (Key::Num1, a), (Key::Num2, b), (Key::D, dry)] {
        h.press(k);
        let out = pull(&mut player, 4_096);
        at += 4_096;
        assert_eq!(t.position() as usize, at, "no jump on switching");
        // Past the crossfade, the output is that take at the playhead,
        // at its matched loudness.
        let (audio, gain) = &takes[take];
        for i in 2_048..4_096 {
            let want = audio[0][at - 4_096 + i] * gain;
            assert!((out[i] - want).abs() < 1e-5, "{k:?} frame {i}: {} vs {want}", out[i]);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_blind_test_ends_in_plain_words_and_is_logged() {
    let dir = temp("blind");
    let mut h = Harness::new(variants(&dir));
    h.until_live();
    let l = h.voicing.live.as_mut().unwrap();
    l.open_blind();
    assert_eq!(l.blind.as_ref().unwrap().pair, (1, 0), "A vs B by default");
    h.frame(Vec::new()); // the chooser is drawn
    let l = h.voicing.live.as_mut().unwrap();
    let dry = l.dry;
    let a = l.letters[0];
    l.blind.as_mut().unwrap().pair = (dry, a); // Dry vs A
    l.start_blind();
    assert!(l.transport.is_playing(), "the first mystery sound plays");
    for round in 0..6 {
        h.frame(Vec::new());
        let (x, a_take) = {
            let l = h.voicing.live.as_ref().unwrap();
            let run = l.blind.as_ref().unwrap().run.as_ref().unwrap();
            assert_eq!(l.transport.selected(), run.take_for(AbxListen::X), "round {round}: the mystery plays");
            (run.take_for(AbxListen::X), run.take_for(AbxListen::A))
        };
        // Hear A and B, then answer correctly from the keyboard.
        h.press(Key::Num1);
        h.press(Key::Num2);
        h.press(if x == a_take { Key::ArrowLeft } else { Key::ArrowRight });
    }
    h.frame(Vec::new()); // the result is drawn
    let l = h.voicing.live.as_ref().unwrap();
    let run = l.blind.as_ref().unwrap().run.as_ref().unwrap();
    assert!(run.finished());
    let stats = run.stats();
    assert_eq!((stats.correct, stats.trials), (6, 6));
    let (headline, detail, _) = super::simple::verdict_words(stats.verdict, stats.correct, stats.trials);
    assert_eq!(headline, "You could tell them apart.");
    assert!(detail.starts_with("6 of 6 right: unlikely to be chance"), "{detail}");
    let (_, detail, _) = super::simple::verdict_words(
        stave_measure::listening::Abx { trials: 16, correct: 14 }.verdict(16),
        14,
        16,
    );
    assert!(detail.starts_with("14 of 16 right: very unlikely to be chance"), "{detail}");
    assert_eq!(l.history.abx.len(), 1);
    assert_eq!(l.history.abx[0].b, "peak 9");
    assert_eq!(l.history.abx[0].a, "dry");
    let (headline, _, _) = super::simple::verdict_words(
        stave_measure::listening::Abx { trials: 16, correct: 9 }.verdict(16),
        9,
        16,
    );
    assert_eq!(headline, "You couldn't reliably tell them apart.");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn which_sounds_better_saves_a_b_tie_and_unclear_apart() {
    let dir = temp("prefer");
    let mut h = Harness::new(variants(&dir));
    h.until_live();
    let first_material = h.voicing.live.as_ref().unwrap().prepared.material.name.clone();
    let d = dir.clone();
    let l = h.voicing.live.as_mut().unwrap();
    l.pref_notes = "more open".into();
    let (a, b) = (l.letters[0], l.letters[1]);
    l.prefer(Choice::A, Some(a), &d);
    l.prefer(Choice::B, Some(b), &d);
    l.prefer(Choice::Tie, None, &d);
    l.prefer(Choice::Unclear, None, &d);
    assert_eq!(l.unclear, Some(3), "the reason field edits the unclear answer");
    l.history.preferences[3].notes = "the clip is too quiet".into();
    h.frame(Vec::new()); // the "try another clip" panel is drawn
    let path = h.voicing.live.as_ref().unwrap().saved.as_ref().unwrap().0.clone();

    // "Try another clip": the same candidates over the next material.
    let next = h.voicing.next_material();
    h.voicing.change_material(next.clone());
    assert!(h.voicing.live.is_none());
    h.until_live();
    assert_eq!(h.voicing.setup.material, next);
    assert_ne!(h.voicing.live.as_ref().unwrap().prepared.material.name, first_material);

    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let prefs = json["preferences"].as_array().unwrap();
    let choices: Vec<&str> = prefs.iter().map(|p| p["choice"].as_str().unwrap()).collect();
    assert_eq!(choices, ["a", "b", "tie", "unclear"]);
    assert_eq!(prefs[0]["label"], "peak 9");
    assert_eq!(prefs[0]["notes"], "more open");
    assert_eq!(prefs[1]["label"], "peak 2");
    assert_eq!(prefs[2]["candidate"], serde_json::Value::Null);
    assert_eq!(prefs[3]["notes"], "the clip is too quiet");
    assert_eq!(prefs[3]["material"], first_material.as_str());
    let summary = std::fs::read_to_string(path.with_extension("txt")).unwrap();
    assert!(summary.contains("a 1, b 1, other 0, tie 1, unclear 1"), "{summary}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_bad_candidate_reports_and_stays_in_setup() {
    let dir = temp("bad");
    let mut l = launch(&dir);
    l.candidates[1] = "stage:filter/peak;nonsense=1".into();
    let mut h = Harness::new(l);
    for _ in 0..3 {
        h.frame(Vec::new());
    }
    assert!(h.voicing.live.is_none());
    let (text, error) = h.voicing.status.clone().unwrap();
    assert!(error && text.contains("nonsense"), "{text}");
    // The setup is still drawn and editable.
    h.frame(Vec::new());
}

#[test]
fn launch_errors_name_the_candidate() {
    let mut l = launch(&temp("err"));
    l.candidates.push("x.vst3;drive".into());
    assert!(Voicing::new(&l).err().unwrap().contains("x.vst3;drive"));
    let mut l = launch(&temp("many"));
    l.candidates = (0..10).map(|_| "dry".to_string()).collect();
    assert!(Voicing::new(&l).is_err());
}
