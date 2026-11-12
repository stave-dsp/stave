//! The face: its bindings resolve against the table, and every grab
//! lands where a person aims at it.

#[test]
fn face_binds_resolve() {
    stave_test::face::binds_resolve::<stave_tilt_vst3::Tilt>("tilt");
}

#[test]
fn face_grabs_land() {
    stave_test::face::grabs_land::<stave_tilt_vst3::Tilt>("tilt");
}
