//! A face's bindings, checked against the plugin's table — every
//! plugin with a face mounts this in its `tests/editor.rs`:
//!
//! * every bound ParamId exists, under the name the face was built with;
//! * stepped gestures (toggles, choices, latches, momentary buttons)
//!   bind stepped parameters, and rotaries and pads bind continuous ones;
//! * a `Choose` gesture's step is in range.

use std::sync::Arc;

use stave::controller::{GestureSink, PluginController};
use stave::params::{AnyParam, IndexedParams, ParamId, Params};
use stave::{Plugin, Telemetry};
use stave_face::{Gesture, Runtime};

/// A sink that drops every edit (the checks only read).
struct Nowhere;

impl GestureSink for Nowhere {
    fn begin(&self, _id: ParamId) {}
    fn perform(&self, _id: ParamId, _value: f64) {}
    fn end(&self, _id: ParamId) {}
}

/// `P`'s face and a controller over a fresh instance of its params: what
/// a face check drives.
pub fn face_of<P: Plugin>(project: &str) -> (Runtime, PluginController) {
    let params = Arc::new(P::Params::default());
    let face = P::face(&params).unwrap_or_else(|| panic!("{project}: declares no face"));
    let presets = P::presets(&params).into();
    let table: Arc<dyn Params> = Arc::new(IndexedParams::new(params));
    let controller = PluginController::new(table, Telemetry::new(), presets, Box::new(Nowhere));
    (Runtime::new(face), controller)
}

/// Check the face of `P` over freshly built params.
pub fn binds_resolve<P: Plugin>(project: &str) {
    let (rt, controller) = face_of::<P>(project);
    let controller: &dyn stave::Controller = &controller;
    let lookup = |id: ParamId| -> &dyn AnyParam {
        controller
            .param(id)
            .unwrap_or_else(|| panic!("{project}: the face binds param id {}, which the table does not declare", id.0))
    };
    for b in rt.binds(controller) {
        let p = lookup(b.id);
        assert_eq!(p.meta().name, b.name, "{project}: param {} is bound under another name", b.id.0);
    }
    for g in rt.gestures(controller) {
        match g {
            Gesture::Rotary { bind, .. } => {
                let p = lookup(bind.id);
                assert!(p.step_count() == 0 || p.step_count() > 8, "{project}: rotary on stepped param `{}`", bind.name);
            }
            Gesture::Pad { x, y, .. } => {
                for b in [x, y] {
                    assert_eq!(lookup(b.id).step_count(), 0, "{project}: pad axis `{}` is stepped", b.name);
                }
            }
            Gesture::Stepped { bind, .. } | Gesture::Latch { bind } | Gesture::Momentary { bind } | Gesture::Nudge { bind, .. } => {
                assert!(lookup(bind.id).step_count() >= 2, "{project}: `{}` needs a stepped param", bind.name);
            }
            Gesture::Choose { bind, step } => {
                let n = lookup(bind.id).step_count();
                assert!(n >= 2, "{project}: `{}` needs a stepped param", bind.name);
                // Toggles offer up to three rows; a two-way toggle's third
                // row is clamped to its last step.
                assert!(step < 3, "{project}: `{}` choice {step} out of range", bind.name);
            }
            Gesture::Preset { .. } | Gesture::Page { .. } => {}
        }
    }
}

/// The face's layout holds where a person aims: every part with a name
/// has a rect inside the face, and a press at the centre of each of its
/// grabs grabs that part with that gesture (nothing on top steals it).
pub fn grabs_land<P: Plugin>(project: &str) {
    let (rt, controller) = face_of::<P>(project);
    let controller: &dyn stave::Controller = &controller;
    let (w, h) = rt.size();
    for (id, r) in rt.parts() {
        assert!(
            r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= w + 0.5 && r.y + r.h <= h + 0.5,
            "{project}: part {id:?} at {r:?} lies outside the {w}×{h} face"
        );
        let Some(id) = id else { continue };
        for grab in rt.grabs(controller, id) {
            let at = grab.area.centre();
            let got = rt.grab_at(controller, at);
            assert_eq!(
                got,
                Some((Some(id), grab.gesture.clone())),
                "{project}: a press at the centre of {id:?}'s {:?} grabs something else",
                grab.gesture
            );
        }
    }
}
