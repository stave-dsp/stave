//! The face in a real (offscreen) AppKit window, driven by real NSEvents
//! (ledger U8): the view's coordinate mapping — the y-flip from AppKit's
//! y-up window to the face's y-down points, the wheel's units, the
//! modifier bits, the click count — is tested, not assumed. The same
//! laws the headless driver checks hold through the platform.
//!
//! AppKit is main-thread only, so this file is its own harness (it
//! answers libtest's `--list` so cargo-nextest runs it too).

#[cfg(target_os = "macos")]
mod real {
    use std::sync::Arc;

    use stave_core::params::{AnyParam, BoolParam, FloatParam, ParamId, Params, Range};
    use stave_core::{GestureSink, PluginController, Telemetry};
    use stave_face::prelude::*;
    use stave_face::{Mods, Runtime};
    use stave_ui_cocoa::synthetic::{Mouse, OffscreenWindow};
    use stave_ui_cocoa::FaceView;

    struct Table {
        knob: FloatParam,
        foot: BoolParam,
    }

    impl Params for Table {
        fn count(&self) -> usize {
            2
        }
        fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
            [&self.knob as &dyn AnyParam, &self.foot].get(i).copied()
        }
    }

    struct Nowhere;
    impl GestureSink for Nowhere {
        fn begin(&self, _: ParamId) {}
        fn perform(&self, _: ParamId, _: f64) {}
        fn end(&self, _: ParamId) {}
    }

    /// A knob at the top, a footswitch below it: a y-flip sends a press
    /// at one to the other.
    fn face(t: &Table) -> Face {
        Face::new(Theme::graphite(), column(vec![Knob::new(t.knob.bind()).node(), Footswitch::new(t.foot.bind()).node()]).gap(24.0))
    }

    struct Rig {
        window: OffscreenWindow,
        knob: (f64, f64),
        foot: (f64, f64),
        h: f64,
    }

    impl Rig {
        fn new() -> Rig {
            let table = Arc::new(Table {
                knob: FloatParam::new(ParamId(1), "Knob", Range::linear(0.0, 1.0)).default(0.5),
                foot: BoolParam::new(ParamId(2), "Foot", false),
            });
            // Where the parts are, from the headless runtime (face
            // points, y down).
            let rt = Runtime::new(face(&table), &stave_ui_cocoa::text::SystemFonts);
            let probe = PluginController::new(table.clone(), Telemetry::new(), Vec::new().into(), Box::new(Nowhere));
            let at = |id| {
                let c = rt.grabs(&probe, PartId::Param(ParamId(id)))[0].area.centre();
                (c.x, c.y)
            };
            let (knob, foot, h) = (at(1), at(2), rt.size().1);
            let controller = PluginController::new(table.clone(), Telemetry::new(), Vec::new().into(), Box::new(Nowhere));
            let window = OffscreenWindow::new(FaceView::new(face(&table), Box::new(controller)));
            Rig { window, knob, foot, h }
        }
        /// A face point (y down) as AppKit's window point (y up).
        fn window_point(&self, (x, y): (f64, f64)) -> (f64, f64) {
            (x, self.h - y)
        }
        fn value(&self, id: u32) -> f64 {
            self.window.view().controller().normalized(ParamId(id))
        }
        fn mouse(&self, kind: Mouse, face_point: (f64, f64), mods: Mods, clicks: i64) {
            self.window.mouse(kind, self.window_point(face_point), mods, clicks);
        }
    }

    pub fn a_click_lands_on_the_part_under_it() {
        let r = Rig::new();
        r.mouse(Mouse::Down, r.foot, Mods::default(), 1);
        r.mouse(Mouse::Up, r.foot, Mods::default(), 1);
        assert_eq!(r.value(2), 1.0, "the footswitch, not what a flipped y would hit");
        assert_eq!(r.value(1), 0.5, "the knob is untouched");
    }

    pub fn dragging_up_on_screen_raises_the_knob() {
        let r = Rig::new();
        let (x, y) = r.knob;
        r.mouse(Mouse::Down, r.knob, Mods::default(), 1);
        r.mouse(Mouse::Dragged, (x, y - 55.0), Mods::default(), 1);
        r.mouse(Mouse::Up, (x, y - 55.0), Mods::default(), 1);
        assert!((r.value(1) - 0.75).abs() < 1e-9, "55 pt up of 220 travel from 0.5: {}", r.value(1));
    }

    pub fn shift_is_fine_and_option_resets() {
        let r = Rig::new();
        let (x, y) = r.knob;
        let fine = Mods { fine: true, ..Mods::default() };
        r.mouse(Mouse::Down, r.knob, fine, 1);
        r.mouse(Mouse::Dragged, (x, y - 55.0), fine, 1);
        r.mouse(Mouse::Up, (x, y - 55.0), fine, 1);
        assert!((r.value(1) - 0.525).abs() < 1e-9, "Shift is ten times finer: {}", r.value(1));
        r.mouse(Mouse::Down, r.knob, Mods { reset: true, ..Mods::default() }, 1);
        r.mouse(Mouse::Up, r.knob, Mods::default(), 1);
        assert_eq!(r.value(1), 0.5, "Option-click resets");
    }

    pub fn a_double_click_resets() {
        let r = Rig::new();
        let (x, y) = r.knob;
        r.mouse(Mouse::Down, r.knob, Mods::default(), 1);
        r.mouse(Mouse::Dragged, (x, y - 30.0), Mods::default(), 1);
        r.mouse(Mouse::Up, (x, y - 30.0), Mods::default(), 1);
        assert_ne!(r.value(1), 0.5);
        r.mouse(Mouse::Down, r.knob, Mods::default(), 2);
        r.mouse(Mouse::Up, r.knob, Mods::default(), 2);
        assert_eq!(r.value(1), 0.5, "AppKit's click count of 2 resets");
    }

    pub fn a_wheel_notch_up_nudges_the_knob_up() {
        let r = Rig::new();
        r.window.wheel(r.window_point(r.knob), 3);
        assert!((r.value(1) - 0.53).abs() < 1e-9, "three line notches up: {}", r.value(1));
        r.window.wheel(r.window_point(r.foot), 3);
        assert!((r.value(1) - 0.53).abs() < 1e-9, "the wheel over another part leaves the knob");
    }

    /// A session in the real window, recorded by the view, replays
    /// headless to the same values (ledger U9).
    pub fn a_recorded_session_replays_to_the_same_values() {
        let path = std::env::temp_dir().join(format!("stave-face-trace-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::env::set_var("STAVE_FACE_TRACE", &path);
        let r = Rig::new();
        std::env::remove_var("STAVE_FACE_TRACE");
        let (x, y) = r.knob;
        r.mouse(Mouse::Moved, r.knob, Mods::default(), 0);
        r.mouse(Mouse::Down, r.knob, Mods::default(), 1);
        r.mouse(Mouse::Dragged, (x + 3.0, y - 41.0), Mods::default(), 1);
        r.mouse(Mouse::Up, (x + 3.0, y - 41.0), Mods::default(), 1);
        r.window.wheel(r.window_point(r.knob), -2);
        r.mouse(Mouse::Down, r.foot, Mods::default(), 1);
        r.mouse(Mouse::Up, r.foot, Mods::default(), 1);
        let (knob, foot) = (r.value(1), r.value(2));
        assert!(knob != 0.5 && foot == 1.0, "the session moved things: {knob} {foot}");
        drop(r);
        let text = std::fs::read_to_string(&path).expect("the view wrote a trace");
        let _ = std::fs::remove_file(&path);
        let trace: stave_face::Trace = text.parse().expect("the trace reads back");
        assert!(trace.0.len() >= 8, "every event recorded:\n{text}");
        let table = Arc::new(Table {
            knob: FloatParam::new(ParamId(1), "Knob", Range::linear(0.0, 1.0)).default(0.5),
            foot: BoolParam::new(ParamId(2), "Foot", false),
        });
        let c = PluginController::new(table.clone(), Telemetry::new(), Vec::new().into(), Box::new(Nowhere));
        let mut rt = Runtime::new(face(&table), &stave_ui_cocoa::text::SystemFonts);
        for act in &trace.0 {
            rt.apply(&c, act);
        }
        assert_eq!((table.knob.normalized(), table.foot.normalized()), (knob, foot), "replayed:\n{text}");
    }

    /// The view's snapshot is the face drawn offscreen at the asked
    /// scale (what `STAVE_FACE_SNAPSHOT` and stave-play's editor
    /// screenshots write), not a capture of the screen.
    pub fn a_snapshot_is_the_face_at_its_scale() {
        let r = Rig::new();
        let png = r.window.view().snapshot(2.0);
        assert_eq!(&png[1..4], b"PNG");
        let (w, h) = r.window.view().size();
        let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as f64;
        assert_eq!((size(16), size(20)), ((w * 2.0).round(), (h * 2.0).round()));
    }

    pub const TESTS: &[(&str, fn())] = &[
        ("a_snapshot_is_the_face_at_its_scale", a_snapshot_is_the_face_at_its_scale),
        ("a_recorded_session_replays_to_the_same_values", a_recorded_session_replays_to_the_same_values),
        ("a_click_lands_on_the_part_under_it", a_click_lands_on_the_part_under_it),
        ("dragging_up_on_screen_raises_the_knob", dragging_up_on_screen_raises_the_knob),
        ("shift_is_fine_and_option_resets", shift_is_fine_and_option_resets),
        ("a_double_click_resets", a_double_click_resets),
        ("a_wheel_notch_up_nudges_the_knob_up", a_wheel_notch_up_nudges_the_knob_up),
    ];
}

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    if flag("--list") {
        if !flag("--ignored") {
            for (name, _) in real::TESTS {
                println!("{name}: test");
            }
        }
        return;
    }
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let exact = flag("--exact");
    let chosen = real::TESTS.iter().filter(|(name, _)| {
        filters.is_empty() || filters.iter().any(|f| if exact { name == f } else { name.contains(f.as_str()) })
    });
    let mut failed = 0;
    let mut ran = 0;
    for (name, test) in chosen {
        ran += 1;
        match std::panic::catch_unwind(test) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!("\ntest result: {}. {} passed; {failed} failed", if failed == 0 { "ok" } else { "FAILED" }, ran - failed);
    if failed > 0 {
        std::process::exit(101);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
