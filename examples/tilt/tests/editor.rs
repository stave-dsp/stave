//! The face's bindings resolve against the table, each control on a
//! parameter of the right kind.

#[test]
fn face_binds_resolve() {
    stave_test::face::binds_resolve::<stave_tilt_vst3::Tilt>("tilt");
}
