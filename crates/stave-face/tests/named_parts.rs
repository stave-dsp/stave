//! Parts are addressed by name, not coordinates (`PartId`): a test asks
//! for a part's rect and grabs, and aims at the centre of a grab. The
//! same names drive visibility by a parameter's step (`on_step`), and
//! grabs follow live values (a toggle's rows follow its step count).

use std::sync::Arc;

use stave_core::params::{AnyParam, BoolParam, EnumParam, FloatParam, ParamId, Params, Range};
use stave_core::{param_enum, Controller, GestureSink, PluginController, Telemetry};
use stave_face::prelude::*;
use stave_face::{Gesture, Mods, Runtime};

param_enum! {
    pub const ALGOS: Algo = ["PLATE", "HALL"];
}

struct Table {
    size: FloatParam,
    algo: EnumParam<Algo>,
    hold: BoolParam,
}

impl Params for Table {
    fn count(&self) -> usize {
        3
    }
    fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
        [&self.size as &dyn AnyParam, &self.algo, &self.hold].get(index).copied()
    }
}

struct Nowhere;
impl GestureSink for Nowhere {
    fn begin(&self, _: ParamId) {}
    fn perform(&self, _: ParamId, _: f64) {}
    fn end(&self, _: ParamId) {}
}

/// A selector, and under it one control per algorithm: the size knob
/// for PLATE, the hold switch for HALL.
fn rig() -> (Runtime, Arc<Table>, PluginController) {
    let table = Arc::new(Table {
        size: FloatParam::new(ParamId(1), "Size", Range::linear(0.0, 1.0)),
        algo: EnumParam::new(ParamId(2), "Algo", Algo(0)),
        hold: BoolParam::new(ParamId(3), "Hold", false),
    });
    let algo = table.algo.bind();
    let face = Face::new(
        Theme::graphite(),
        column(vec![
            Toggle::new(algo).node(),
            stack(vec![
                on_step(Knob::new(table.size.bind()).node(), algo, 0),
                on_step(Footswitch::new(table.hold.bind()).node().id("hold"), algo, 1),
            ]),
            Title::new("ROOM").node(),
        ]),
    );
    let controller = PluginController::new(table.clone(), Telemetry::new(), Vec::new().into(), Box::new(Nowhere));
    (Runtime::new(face, &stave_raster::BoxText), table, controller)
}

#[test]
fn parts_are_found_by_param_name_role_or_node_id() {
    let (rt, _, c) = rig();
    assert_eq!(rt.find(&c, "size"), Some(PartId::Param(ParamId(1))));
    assert_eq!(rt.find(&c, "Algo"), Some(PartId::Param(ParamId(2))));
    assert_eq!(rt.find(&c, "hold"), Some(PartId::Name("hold")), "a node id names the part");
    assert_eq!(rt.find(&c, "title"), Some(PartId::Name("title")), "a role names the part");
    assert_eq!(rt.find(&c, "nothing"), None);
    let title = rt.rect(PartId::Name("title")).expect("the title has a rect");
    let knob = rt.rect(PartId::Param(ParamId(1))).expect("the knob has a rect");
    assert!(title.y > knob.y, "laid out in column order");
}

#[test]
fn a_press_at_a_grabs_centre_grabs_that_part() {
    let (rt, _, c) = rig();
    let knob = PartId::Param(ParamId(1));
    let grabs = rt.grabs(&c, knob);
    assert_eq!(grabs.len(), 1);
    let at = grabs[0].area.centre();
    let (id, gesture) = rt.grab_at(&c, at).expect("the centre grabs");
    assert_eq!(id, Some(knob));
    assert!(matches!(gesture, Gesture::Rotary { .. }));
}

#[test]
fn content_follows_its_selectors_step() {
    let (mut rt, table, c) = rig();
    let (knob, hold) = (PartId::Param(ParamId(1)), PartId::Name("hold"));
    assert_eq!(rt.grabs(&c, knob).len(), 1, "PLATE shows the size knob");
    assert!(rt.grabs(&c, hold).is_empty(), "and hides the hold switch");
    // Choose HALL on the toggle's second row.
    let algo = rt.grabs(&c, PartId::Param(ParamId(2)));
    let hall = algo
        .iter()
        .find(|g| g.gesture == Gesture::Choose { bind: table.algo.bind(), step: 1 })
        .expect("a row per option");
    assert!(rt.pointer_down(&c, hall.area.centre(), Mods::default(), 1));
    rt.pointer_up(&c);
    assert_eq!(table.algo.value(), Algo(1));
    assert!(rt.grabs(&c, knob).is_empty(), "HALL hides the size knob");
    let switch = rt.grabs(&c, hold);
    assert_eq!(switch.len(), 1, "and shows the hold switch");
    let (id, _) = rt.grab_at(&c, switch[0].area.centre()).expect("grabbable where it shows");
    assert_eq!(id, Some(hold));
}

#[test]
fn a_toggles_rows_follow_its_step_count() {
    let (rt, _, c) = rig();
    let rows = rt
        .grabs(&c, PartId::Param(ParamId(2)))
        .into_iter()
        .filter(|g| matches!(g.gesture, Gesture::Choose { .. }))
        .count();
    assert_eq!(rows, 2, "a two-way toggle offers two rows, not three");
    let _ = (&c as &dyn Controller).steps(ParamId(2));
}
