//! The Audio Unit path end to end on an effect: Tilt, linked in-process,
//! registered with AudioToolbox and driven through it (see `au_common`).

#![cfg(target_os = "macos")]

mod au_common;

use au_common::{energy, BLOCK};
use stave_host::au::AuInstance;
use stave_host::instance::Instance;
use stave_host::param::find;

fn tilt() -> AuInstance {
    au_common::open(b"aufx", "stave tests: Tilt", stave_tilt_vst3::StaveAUFactory)
}

#[test]
fn opens_with_its_params() {
    let t = tilt();
    let l = t.layout();
    assert!(!l.is_instrument());
    assert!((1..=2).contains(&l.outputs) && l.inputs == l.outputs, "{l:?}");
    let params = t.params();
    let tilt = find(&params, "tilt").expect("tilt param");
    assert_eq!((tilt.min, tilt.max, tilt.default), (-6.0, 6.0, 0.0));
    assert_eq!(tilt.unit, "dB");
    let bypass = find(&params, "bypass").expect("bypass param");
    assert_eq!(bypass.step_count, 2, "a switch: {bypass:?}");
}

#[test]
fn processes_what_its_params_say() {
    let mut t = tilt();
    t.set_param("output", -6.0).unwrap();
    let ch = t.layout().outputs;
    let x = vec![0.5f32; BLOCK];
    let mut outs = vec![vec![0.0f32; BLOCK]; ch];
    for _ in 0..200 {
        let ins: Vec<&[f32]> = (0..ch).map(|_| &x[..]).collect();
        let mut o: Vec<&mut [f32]> = outs.iter_mut().map(|v| &mut v[..]).collect();
        t.process(&ins, &mut o, &[]);
    }
    // Flat tilt, -6 dB: DC through at half amplitude.
    let y = outs[0][BLOCK - 1];
    assert!((y - 0.5 * 0.501_187).abs() < 1e-3, "{y}");
    assert!(energy(&outs[0]) > 0.0);
}

#[test]
fn state_round_trips_through_class_info() {
    let mut a = tilt();
    a.set_param("tilt", 3.5).unwrap();
    let blob = a.save_state().unwrap();
    let mut b = tilt();
    b.load_state(&blob).unwrap();
    let restored = find(&b.params(), "tilt").unwrap().value;
    assert!((restored - 3.5).abs() < 1e-6, "{restored}");
    assert!(b.load_state(b"not a property list").is_err());
}

/// The face is offered as a Cocoa view: a bundle that exists and a class
/// the Objective-C runtime knows.
#[test]
fn offers_its_face_as_a_cocoa_view() {
    let (bundle, class) = tilt().cocoa_view().expect("Tilt has a face, so a Cocoa view");
    assert!(std::path::Path::new(&bundle).exists(), "{bundle}");
    assert!(class.starts_with("StaveViewFactory_"), "{class}");
}

#[test]
fn passes_the_validation_suite() {
    au_common::passes_the_validation_suite(tilt);
}
