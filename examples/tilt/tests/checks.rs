//! Every check Tilt owes (validation, no allocation, perf, the face's
//! laws, host behaviour), from one line. Perf baseline 2026-07-06 (Apple
//! Silicon, ns/frame): FLAT ~4 (true bypass), FULL TILT ~12 (two biquads
//! per lane).

stave_test::plugin_checks!(stave_tilt_vst3::Tilt, {
    perf: vec![PerfCase { name: "FLAT", set: &[("Tilt", 0.0)] }, PerfCase { name: "FULL TILT", set: &[("Tilt", 6.0)] }],
});
