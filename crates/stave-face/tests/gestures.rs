//! The runtime owns every gesture; these pin what each one does, on a
//! face laid out and hit-tested for real (no renderer needed).

use std::collections::HashMap;

use stave_face::prelude::*;
use stave_face::{step_of, step_value, Host, Mods, Runtime};
use stave_ui::scene::{pt, Pt};

#[derive(Default)]
struct Table {
    v: HashMap<u32, f64>,
    steps: HashMap<u32, u32>,
    log: Vec<(&'static str, u32)>,
}

impl Host for Table {
    fn normalized(&self, id: u32) -> f64 {
        *self.v.get(&id).unwrap_or(&0.0)
    }
    fn default_normalized(&self, _id: u32) -> f64 {
        0.25
    }
    fn steps(&self, id: u32) -> u32 {
        *self.steps.get(&id).unwrap_or(&0)
    }
    fn step_name(&self, _id: u32, i: u32) -> Option<String> {
        Some(format!("S{i}"))
    }
    fn value_text(&self, id: u32) -> String {
        format!("{:.2}", self.normalized(id))
    }
    fn probe(&self, _name: &str) -> f64 {
        0.0
    }
    fn begin(&mut self, id: u32) {
        self.log.push(("begin", id));
    }
    fn set(&mut self, id: u32, n: f64) {
        let n = match self.steps(id) {
            0 => n,
            k => step_value(step_of(n, k), k),
        };
        self.v.insert(id, n.clamp(0.0, 1.0));
    }
    fn end(&mut self, id: u32) {
        self.log.push(("end", id));
    }
}

const KNOB: Bind = Bind::new(1, "KNOB");
const TOGGLE: Bind = Bind::new(2, "TOGGLE");
const FOOT: Bind = Bind::new(3, "FOOT");

/// A face with one of each control, and their centres.
fn rig() -> (Runtime, Table, Pt, Pt, Pt) {
    let face = Face::new(
        Theme::graphite(),
        row(vec![Knob::new(KNOB).node(), Toggle::new(TOGGLE).node(), Footswitch::new(FOOT).node()]),
    );
    let rt = Runtime::new(face);
    let r = rt.rects();
    let knob = pt(r[0].x + r[0].w / 2.0, r[0].y + 21.0 + 7.0 + 5.0);
    let toggle = pt(r[1].x + 28.0, r[1].y + 46.0);
    let foot = pt(r[2].x + r[2].w / 2.0, r[2].y + r[2].h - 24.0);
    let mut t = Table::default();
    t.v.insert(KNOB.id, 0.5);
    t.steps.insert(TOGGLE.id, 3);
    t.steps.insert(FOOT.id, 2);
    (rt, t, knob, toggle, foot)
}

#[test]
fn rotary_drag_sweeps_the_range_over_its_travel_and_brackets_the_edit() {
    let (mut rt, mut t, k, _, _) = rig();
    assert!(rt.pointer_down(&mut t, k, Mods::default(), 1));
    rt.pointer_drag(&mut t, pt(k.x, k.y - 110.0), Mods::default());
    assert!((t.normalized(KNOB.id) - 1.0).abs() < 1e-9, "half the travel up from 0.5 reaches the top");
    rt.pointer_drag(&mut t, pt(k.x, k.y + 500.0), Mods::default());
    assert_eq!(t.normalized(KNOB.id), 0.0, "clamped at the bottom");
    rt.pointer_up(&mut t);
    assert_eq!(t.log, vec![("begin", KNOB.id), ("end", KNOB.id)]);
}

#[test]
fn shift_is_ten_times_finer_and_re_anchors_without_a_jump() {
    let (mut rt, mut t, k, _, _) = rig();
    rt.pointer_down(&mut t, k, Mods::default(), 1);
    rt.pointer_drag(&mut t, pt(k.x, k.y - 22.0), Mods::default());
    let coarse = t.normalized(KNOB.id);
    assert!((coarse - 0.6).abs() < 1e-9);
    let fine = Mods { fine: true, ..Mods::default() };
    rt.pointer_drag(&mut t, pt(k.x, k.y - 22.0), fine);
    assert!((t.normalized(KNOB.id) - coarse).abs() < 1e-12, "toggling Shift never jumps");
    rt.pointer_drag(&mut t, pt(k.x, k.y - 44.0), fine);
    assert!((t.normalized(KNOB.id) - (coarse + 0.01)).abs() < 1e-9);
}

#[test]
fn double_click_and_alt_click_reset_to_default() {
    let (mut rt, mut t, k, _, _) = rig();
    rt.pointer_down(&mut t, k, Mods::default(), 2);
    assert_eq!(t.normalized(KNOB.id), 0.25);
    t.v.insert(KNOB.id, 0.9);
    rt.pointer_down(&mut t, k, Mods { reset: true, ..Mods::default() }, 1);
    assert_eq!(t.normalized(KNOB.id), 0.25);
}

#[test]
fn the_wheel_nudges_a_rotary_and_steps_a_toggle() {
    let (mut rt, mut t, k, tg, _) = rig();
    rt.wheel(&mut t, k, 3.0, Mods::default());
    assert!((t.normalized(KNOB.id) - 0.53).abs() < 1e-9);
    rt.wheel(&mut t, tg, 1.0, Mods::default());
    assert_eq!(step_of(t.normalized(TOGGLE.id), 3), 1);
    rt.wheel(&mut t, tg, -1.0, Mods::default());
    rt.wheel(&mut t, tg, -1.0, Mods::default());
    assert_eq!(step_of(t.normalized(TOGGLE.id), 3), 0, "clamped, no wrap on the wheel");
}

#[test]
fn a_toggle_click_steps_forward_and_wraps_and_its_options_choose() {
    let (mut rt, mut t, _, tg, _) = rig();
    for want in [1, 2, 0] {
        rt.pointer_down(&mut t, tg, Mods::default(), 1);
        rt.pointer_up(&mut t);
        assert_eq!(step_of(t.normalized(TOGGLE.id), 3), want);
    }
    // The last printed option, right of the pivot.
    rt.pointer_down(&mut t, pt(tg.x + 30.0, tg.y + 13.0), Mods::default(), 1);
    assert_eq!(step_of(t.normalized(TOGGLE.id), 3), 2);
}

#[test]
fn a_footswitch_latches() {
    let (mut rt, mut t, _, _, f) = rig();
    rt.pointer_down(&mut t, f, Mods::default(), 1);
    rt.pointer_up(&mut t);
    assert_eq!(t.normalized(FOOT.id), 1.0);
    rt.pointer_down(&mut t, f, Mods::default(), 1);
    assert_eq!(t.normalized(FOOT.id), 0.0);
}

#[test]
fn hover_sets_the_cursor_and_misses_change_nothing() {
    let (mut rt, mut t, k, _, f) = rig();
    assert!(rt.pointer_move(k));
    assert_eq!(rt.cursor(), stave_face::Cursor::Grab);
    rt.pointer_move(f);
    assert_eq!(rt.cursor(), stave_face::Cursor::Point);
    rt.pointer_move(pt(-50.0, -50.0));
    assert_eq!(rt.cursor(), stave_face::Cursor::Arrow);
    assert!(!rt.pointer_down(&mut t, pt(-50.0, -50.0), Mods::default(), 1));
    assert!(t.log.is_empty());
}
