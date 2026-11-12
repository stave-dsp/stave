//! A face's bindings, checked against the plugin's table — every
//! plugin with a face mounts this in its `tests/editor.rs`:
//!
//! * every bound ParamId exists, under the name the face was built with;
//! * stepped gestures (toggles, choices, latches, momentary buttons)
//!   bind stepped parameters, and rotaries and pads bind continuous ones;
//! * a `Choose` gesture's step is in range.

use std::sync::Arc;

use stave::params::{AnyParam, IndexedParams, ParamId, Params};
use stave::Plugin;
use stave_face::{Gesture, Runtime};

/// Check the face of `P` over freshly built params.
pub fn binds_resolve<P: Plugin>(project: &str) {
    let params = Arc::new(P::Params::default());
    let face = P::face(&params).unwrap_or_else(|| panic!("{project}: declares no face"));
    let table: Arc<dyn Params> = Arc::new(IndexedParams::new(params));
    let rt = Runtime::new(face);
    let lookup = |id: ParamId| -> &dyn AnyParam {
        table
            .by_id(id)
            .unwrap_or_else(|| panic!("{project}: the face binds param id {}, which the table does not declare", id.0))
    };
    for b in rt.binds() {
        let p = lookup(b.id);
        assert_eq!(p.meta().name, b.name, "{project}: param {} is bound under another name", b.id.0);
    }
    for g in rt.gestures() {
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
