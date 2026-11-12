//! The runtime owns every gesture; these pin what each one does, on a
//! face laid out and hit-tested for real (no renderer needed).

use std::sync::{Arc, Mutex};

use stave_core::params::{AnyParam, BoolParam, EnumParam, FloatParam, ParamId, Params, Range};
use stave_core::{param_enum, Controller, GestureSink, PluginController, Telemetry};
use stave_face::prelude::*;
use stave_face::{step_of, Mods, Runtime};
use stave_ui::scene::{pt, Pt};

param_enum! {
    pub const MODES: Mode = ["A", "B", "C"];
}

/// One continuous, one three-way and one two-way parameter.
struct Table {
    knob: FloatParam,
    toggle: EnumParam<Mode>,
    foot: BoolParam,
}

impl Params for Table {
    fn count(&self) -> usize {
        3
    }
    fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
        match index {
            0 => Some(&self.knob),
            1 => Some(&self.toggle),
            2 => Some(&self.foot),
            _ => None,
        }
    }
}

/// The gestures the controller passed on, in order.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(&'static str, ParamId)>>>);

impl GestureSink for Log {
    fn begin(&self, id: ParamId) {
        self.0.lock().unwrap().push(("begin", id));
    }
    fn perform(&self, _id: ParamId, _plain: f64) {}
    fn end(&self, id: ParamId) {
        self.0.lock().unwrap().push(("end", id));
    }
}

impl Log {
    fn take(&self) -> Vec<(&'static str, ParamId)> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

/// The face's controller over the table, and a view of its values.
struct Rig {
    table: Arc<Table>,
    log: Log,
    controller: PluginController,
}

impl Rig {
    fn normalized(&self, b: Bind) -> f64 {
        (&self.controller as &dyn Controller).normalized(b.id)
    }
}

const KNOB: Bind = Bind::new(ParamId(1), "KNOB");
const TOGGLE: Bind = Bind::new(ParamId(2), "TOGGLE");
const FOOT: Bind = Bind::new(ParamId(3), "FOOT");

/// A face with one of each control, and their centres.
fn rig() -> (Runtime, Rig, Pt, Pt, Pt) {
    let face = Face::new(
        Theme::graphite(),
        row(vec![Knob::new(KNOB).node(), Toggle::new(TOGGLE).node(), Footswitch::new(FOOT).node()]),
    );
    let rt = Runtime::new(face, &stave_raster::BoxText);
    let r = rt.rects();
    let knob = pt(r[0].x + r[0].w / 2.0, r[0].y + 21.0 + 7.0 + 5.0);
    let toggle = pt(r[1].x + 28.0, r[1].y + 46.0);
    let foot = pt(r[2].x + r[2].w / 2.0, r[2].y + r[2].h - 24.0);
    let table = Arc::new(Table {
        knob: FloatParam::new(ParamId(1), "KNOB", Range::linear(0.0, 1.0)).default(0.25),
        toggle: EnumParam::new(ParamId(2), "TOGGLE", Mode(0)),
        foot: BoolParam::new(ParamId(3), "FOOT", false),
    });
    table.knob.set_normalized(0.5);
    let log = Log::default();
    let controller = PluginController::new(table.clone(), Telemetry::new(), Vec::new().into(), Box::new(log.clone()));
    (rt, Rig { table, log, controller }, knob, toggle, foot)
}

#[test]
fn rotary_drag_sweeps_the_range_over_its_travel_and_brackets_the_edit() {
    let (mut rt, t, k, _, _) = rig();
    assert!(rt.pointer_down(&t.controller, k, Mods::default(), 1));
    rt.pointer_drag(&t.controller, pt(k.x, k.y - 110.0), Mods::default());
    assert!((t.normalized(KNOB) - 1.0).abs() < 1e-9, "half the travel up from 0.5 reaches the top");
    rt.pointer_drag(&t.controller, pt(k.x, k.y + 500.0), Mods::default());
    assert_eq!(t.normalized(KNOB), 0.0, "clamped at the bottom");
    rt.pointer_up(&t.controller);
    assert_eq!(t.log.take(), vec![("begin", KNOB.id), ("end", KNOB.id)]);
}

#[test]
fn shift_is_ten_times_finer_and_re_anchors_without_a_jump() {
    let (mut rt, t, k, _, _) = rig();
    rt.pointer_down(&t.controller, k, Mods::default(), 1);
    rt.pointer_drag(&t.controller, pt(k.x, k.y - 22.0), Mods::default());
    let coarse = t.normalized(KNOB);
    assert!((coarse - 0.6).abs() < 1e-9);
    let fine = Mods { fine: true, ..Mods::default() };
    rt.pointer_drag(&t.controller, pt(k.x, k.y - 22.0), fine);
    assert!((t.normalized(KNOB) - coarse).abs() < 1e-12, "toggling Shift never jumps");
    rt.pointer_drag(&t.controller, pt(k.x, k.y - 44.0), fine);
    assert!((t.normalized(KNOB) - (coarse + 0.01)).abs() < 1e-9);
}

#[test]
fn double_click_and_alt_click_reset_to_default() {
    let (mut rt, t, k, _, _) = rig();
    rt.pointer_down(&t.controller, k, Mods::default(), 2);
    assert_eq!(t.normalized(KNOB), 0.25);
    t.table.knob.set_normalized(0.9);
    rt.pointer_down(&t.controller, k, Mods { reset: true, ..Mods::default() }, 1);
    assert_eq!(t.normalized(KNOB), 0.25);
}

#[test]
fn the_wheel_nudges_a_rotary_and_steps_a_toggle() {
    let (mut rt, t, k, tg, _) = rig();
    rt.wheel(&t.controller, k, 3.0, Mods::default());
    assert!((t.normalized(KNOB) - 0.53).abs() < 1e-9);
    rt.wheel(&t.controller, tg, 1.0, Mods::default());
    assert_eq!(step_of(t.normalized(TOGGLE), 3), 1);
    rt.wheel(&t.controller, tg, -1.0, Mods::default());
    rt.wheel(&t.controller, tg, -1.0, Mods::default());
    assert_eq!(step_of(t.normalized(TOGGLE), 3), 0, "clamped, no wrap on the wheel");
}

#[test]
fn a_toggle_click_steps_forward_and_wraps_and_its_options_choose() {
    let (mut rt, t, _, tg, _) = rig();
    for want in [1, 2, 0] {
        rt.pointer_down(&t.controller, tg, Mods::default(), 1);
        rt.pointer_up(&t.controller);
        assert_eq!(step_of(t.normalized(TOGGLE), 3), want);
    }
    // The last printed option, right of the pivot.
    rt.pointer_down(&t.controller, pt(tg.x + 30.0, tg.y + 13.0), Mods::default(), 1);
    assert_eq!(step_of(t.normalized(TOGGLE), 3), 2);
}

#[test]
fn a_footswitch_latches() {
    let (mut rt, t, _, _, f) = rig();
    rt.pointer_down(&t.controller, f, Mods::default(), 1);
    rt.pointer_up(&t.controller);
    assert_eq!(t.normalized(FOOT), 1.0);
    rt.pointer_down(&t.controller, f, Mods::default(), 1);
    assert_eq!(t.normalized(FOOT), 0.0);
}

#[test]
fn hover_sets_the_cursor_and_misses_change_nothing() {
    let (mut rt, t, k, _, f) = rig();
    assert!(rt.pointer_move(&t.controller, k));
    assert_eq!(rt.cursor(), stave_face::Cursor::Grab);
    rt.pointer_move(&t.controller, f);
    assert_eq!(rt.cursor(), stave_face::Cursor::Point);
    rt.pointer_move(&t.controller, pt(-50.0, -50.0));
    assert_eq!(rt.cursor(), stave_face::Cursor::Arrow);
    assert!(!rt.pointer_down(&t.controller, pt(-50.0, -50.0), Mods::default(), 1));
    assert!(t.log.take().is_empty());
}
