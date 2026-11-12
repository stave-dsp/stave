//! A face's bindings, checked against the plugin's table — every
//! plugin with a face mounts this in its `tests/editor.rs`:
//!
//! * every bound ParamId exists, under the name the face was built with;
//! * stepped gestures (toggles, choices, latches, momentary buttons)
//!   bind stepped parameters, and rotaries and pads bind continuous ones;
//! * a `Choose` gesture's step is in range.

use stave_core::face::{Gesture, Runtime};
use stave_core::params::AnyParam;
use stave_core::Plugin;

/// Check the face of a freshly constructed plugin `P`.
pub fn binds_resolve<P: Plugin>(project: &str) {
    let plugin = P::new();
    let face = plugin
        .editor_face()
        .unwrap_or_else(|| panic!("{project}: declares no face"));
    let table = plugin.param_table();
    let rt = Runtime::new(face);
    let lookup = |id: u32| -> &dyn AnyParam {
        (0..table.count())
            .filter_map(|i| table.by_index(i))
            .find(|p| p.meta().id.0 == id)
            .unwrap_or_else(|| panic!("{project}: the face binds param id {id}, which the table does not declare"))
    };
    for b in rt.binds() {
        let p = lookup(b.id);
        assert_eq!(p.meta().name, b.name, "{project}: param {} is bound under another name", b.id);
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
            Gesture::Stepped { bind, .. } | Gesture::Latch { bind } | Gesture::Momentary { bind } => {
                assert!(lookup(bind.id).step_count() >= 2, "{project}: `{}` needs a stepped param", bind.name);
            }
            Gesture::Choose { bind, step } => {
                let n = lookup(bind.id).step_count();
                assert!(n >= 2, "{project}: `{}` needs a stepped param", bind.name);
                // Toggles offer up to three rows; a two-way toggle's third
                // row is clamped to its last step.
                assert!(step < 3, "{project}: `{}` choice {step} out of range", bind.name);
            }
        }
    }
}
