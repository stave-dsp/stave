//! Every check Tilt owes (validation, no allocation, perf, the face's
//! laws, host behaviour), from one line. The perf check holds Tilt to its
//! `CPU_BUDGET` (40 ns a sample a channel). Measured 2026-11-13, CPU ns
//! a frame (two channels): Apple Silicon FLAT 5.3, FULL TILT 12.2;
//! x86_64 Linux FLAT 11.3, FULL TILT 23.0, WORST KNOBS 28.3.

stave_test::plugin_checks!(stave_tilt_vst3::Tilt, {
    perf: vec![PerfCase { name: "FLAT", set: &[("Tilt", 0.0)] }, PerfCase { name: "FULL TILT", set: &[("Tilt", 6.0)] }],
});
