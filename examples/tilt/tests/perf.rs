//! The perf report: tilt's representative configurations measured
//! against the realtime budget (see stave-test::perf for the
//! doctrine — it documents, it does not block). Baseline 2026-07-06
//! (Apple Silicon, ns/frame): FLAT ~4 (true bypass), FULL TILT ~12
//! (two biquads per lane).

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_tilt_vst3::Tilt>(
        "tilt",
        &[
            PerfCase {
                name: "FLAT",
                set: &[("Tilt", 0.0)],
            },
            PerfCase {
                name: "FULL TILT",
                set: &[("Tilt", 6.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
