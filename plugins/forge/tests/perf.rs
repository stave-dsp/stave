//! The perf report: forge's representative configurations measured
//! against the realtime budget (see stave-test::perf for the
//! doctrine — it documents, it does not block). Baselines at
//! adoption (2026-07-06, Apple Silicon): ~670 ns/frame, 3.2% of
//! budget — the nested 4x oversampled M/S strip pair is the line's
//! heaviest engine by design.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_forge_vst3::Forge>(
        "forge",
        &[
            PerfCase {
                name: "DEFAULT",
                set: &[],
            },
            PerfCase {
                name: "HOT",
                set: &[
                    ("drive", 9.0),
                    ("depth", 0.8),
                    ("sub mix", 0.6),
                    ("grit", 0.7),
                ],
            },
        ],
        &PerfConfig::default(),
    );
}
