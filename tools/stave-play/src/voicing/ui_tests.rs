//! The Voicing view driven headlessly: real egui frames with simulated
//! keys, no window and no audio device, through setup, the three modes
//! and the log.

use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;
use eframe::egui::{self, Event, Key, Modifiers};
use stave_host::config::{HostConfig, SampleRate};

use super::trials::AbxRun;
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

    // Sighted: space plays, digits and A/B select.
    h.press(Key::Space);
    assert!(h.voicing.live.as_ref().unwrap().transport.is_playing());
    h.press(Key::Num3);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 2);
    h.press(Key::B);
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 1);
    h.press(Key::Num9); // no ninth candidate: ignored
    assert_eq!(h.voicing.live.as_ref().unwrap().transport.selected(), 1);

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

    // End: the log holds it all.
    let path = h.voicing.live.as_ref().unwrap().saved.as_ref().unwrap().0.clone();
    h.voicing.end_session();
    assert!(h.voicing.live.is_none());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["abx"][0]["stats"]["correct"], 4);
    assert_eq!(json["ratings"][0]["trials"].as_array().unwrap().len(), 1);
    assert_eq!(json["material"]["trim_db"], -3.0);
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
