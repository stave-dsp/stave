//! The face, as a person uses it: Tilt's own controls do what their
//! legends say (the generic face laws run in `checks.rs`).

use stave_test::face::{dy, FaceDriver, Key};
use stave_tilt_vst3::Tilt;

#[test]
fn dragging_the_tilt_knob_up_brightens_and_shows_the_value() {
    let mut ui = FaceDriver::of::<Tilt>();
    ui.press("tilt").move_to(dy(-55.0));
    assert_eq!(ui.value("tilt"), 3.0, "a quarter of the travel is a quarter of the range");
    assert!(ui.shows("3.0 dB"), "the readout shows the value while dragging: {:?}", ui.texts());
    ui.release().exit();
    assert!(!ui.shows("3.0 dB"), "and goes away after");
    ui.assert_bracketed();
}

#[test]
fn the_keyboard_nudges_the_focused_knob() {
    let mut ui = FaceDriver::of::<Tilt>();
    ui.click("output");
    assert_eq!(ui.focus(), Some(ui.part("output")));
    ui.key(Key::End);
    assert_eq!(ui.value("output"), 24.0);
    ui.key(Key::Escape).key(Key::Home);
    assert_eq!(ui.value("output"), 24.0, "Escape lets go of the focus");
    ui.assert_bracketed();
}

#[test]
fn the_bypass_footswitch_latches() {
    let mut ui = FaceDriver::of::<Tilt>();
    ui.click("bypass");
    assert_eq!(ui.value("bypass"), 1.0);
    ui.click("bypass");
    assert_eq!(ui.value("bypass"), 0.0);
}
