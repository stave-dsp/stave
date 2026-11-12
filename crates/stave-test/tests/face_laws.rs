//! The face laws catch what they claim to: faces built wrong on purpose
//! fail them, and the driver's acts reach the face at exact points.

use std::sync::Arc;

use stave::params::{AnyParam, BoolParam, FloatParam, ParamId, Params, Range};
use stave_face::prelude::*;
use stave_face::{leaf, Area, Gesture, Grab, Part};
use stave_test::face::{bracketed, dy, laws, pt, Edit, FaceDriver};
use stave_ui::layout::Rect as R;

struct Table {
    a: FloatParam,
    b: FloatParam,
    on: BoolParam,
}

impl Params for Table {
    fn count(&self) -> usize {
        3
    }
    fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
        [&self.a as &dyn AnyParam, &self.b, &self.on].get(i).copied()
    }
}

fn table() -> Arc<Table> {
    Arc::new(Table {
        a: FloatParam::new(ParamId(1), "A", Range::linear(0.0, 1.0)).default(0.5),
        b: FloatParam::new(ParamId(2), "B", Range::linear(0.0, 1.0)),
        on: BoolParam::new(ParamId(3), "On", false),
    })
}

/// A rotary whose grab reaches `reach` points past its own 40-pt part.
struct Wide {
    bind: Bind,
    reach: f64,
}

impl Part for Wide {
    fn grabs(&self, r: R, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        out.push(Grab {
            area: Area::Rect(R { x: r.x - self.reach, y: r.y, w: r.w + 2.0 * self.reach, h: r.h }),
            gesture: Gesture::Rotary { bind: self.bind, travel: 100.0 },
        });
    }
    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
    }
}

fn driver(face: FaceNode) -> FaceDriver {
    let t = table();
    FaceDriver::new(Face::new(Theme::graphite(), face), t as Arc<dyn Params>, Vec::new())
}

fn knobs() -> FaceDriver {
    let t = table();
    driver(row(vec![Knob::new(t.a.bind()).node(), Knob::new(t.b.bind()).node(), Footswitch::new(t.on.bind()).node()]))
}

#[test]
fn a_plain_face_keeps_every_law() {
    laws::assert_all("knobs", knobs, &laws::Laws::default());
}

#[test]
#[should_panic(expected = "lies outside its part")]
fn a_grab_reaching_past_its_part_fails_the_layout() {
    let t = table();
    let make = || driver(row(vec![Knob::new(t.b.bind()).node(), leaf(Wide { bind: t.a.bind(), reach: 6.0 }, 40.0, 40.0), Knob::new(t.b.bind()).node()]).gap(20.0));
    laws::layout("wide", &make, &laws::Laws { off_face: &["On"] });
}

#[test]
#[should_panic(expected = "no grab on any page moves \"On\"")]
fn a_parameter_no_grab_moves_fails_unless_declared_off_face() {
    let t = table();
    let make = || driver(row(vec![Knob::new(t.a.bind()).node(), Knob::new(t.b.bind()).node()]));
    laws::layout("partial", &make, &laws::Laws { off_face: &[] });
}

#[test]
fn edits_are_logged_in_plain_units_and_bracketed() {
    let mut ui = knobs();
    ui.drag("a", dy(-110.0));
    let edits = ui.take_edits();
    assert_eq!(edits.first(), Some(&Edit::Begin(ParamId(1))));
    assert_eq!(edits.last(), Some(&Edit::End(ParamId(1))));
    assert!(edits.contains(&Edit::Perform(ParamId(1), 1.0)));
    assert!(bracketed(&edits).is_ok());
    assert!(bracketed(&[Edit::Perform(ParamId(1), 0.0)]).is_err());
    assert!(bracketed(&[Edit::Begin(ParamId(1))]).is_err());
}

#[test]
fn exact_points_hit_and_miss() {
    let ui = knobs();
    let c = ui.at("a");
    assert_eq!(ui.grab_at(c).map(|(p, _)| p), Some(Some(stave_face::PartId::Param(ParamId(1)))));
    assert_eq!(ui.grab_at(pt(-5.0, -5.0)), None);
    let r = ui.rect("b");
    assert!(r.x > c.x, "b is right of a");
}

#[test]
fn a_trace_replays_to_the_same_values() {
    let mut ui = knobs();
    ui.drag("a", dy(-30.0)).wheel("b", 4.0).click("on").double_click("a");
    let trace = ui.trace().to_string();
    let mut again = knobs();
    again.replay(&trace.parse().expect("a trace reads back"));
    for p in ["a", "b", "on"] {
        assert_eq!(again.value(p), ui.value(p), "{p}");
    }
    assert_eq!(again.edits(), ui.edits());
}
