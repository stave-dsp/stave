//! The perf report: representative configurations measured against
//! the realtime budget (stave-test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_tolex_vst3::Tolex>(
        "tolex",
        &[
            PerfCase {
                name: "DEFAULT",
                set: &[],
            },
            PerfCase {
                name: "OPEN BACK",
                set: &[("back", 1.0)],
            },
            PerfCase {
                name: "TUBE + DRIVE",
                set: &[("damping", 1.0), ("drive", 24.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
