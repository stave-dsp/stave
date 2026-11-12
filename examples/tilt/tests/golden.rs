//! Tilt's face as pictures, per interaction state (idle, hovered,
//! dragging with its readout), compared to the goldens in
//! `tests/golden` (`STAVE_BLESS=1` rewrites them after a reviewed change).

use stave_test::assert_golden;
use stave_test::face::{dy, FaceDriver};
use stave_tilt_vst3::Tilt;

#[test]
#[cfg_attr(miri, ignore)]
fn the_face_looks_as_it_should_in_every_state() {
    let mut ui = FaceDriver::of::<Tilt>();
    assert_golden!("tilt-idle", &ui.render(1.0));
    ui.hover("tilt");
    assert_golden!("tilt-hover", &ui.render(1.0));
    ui.press("tilt").move_to(dy(-55.0));
    assert_golden!("tilt-drag", &ui.render(1.0));
}
