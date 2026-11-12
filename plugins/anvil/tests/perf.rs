//! The perf report: representative configurations measured against
//! the realtime budget (stave-test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_anvil_vst3::Anvil>(
        "anvil",
        &[
            PerfCase {
                name: "DEFAULT",
                set: &[],
            },
            PerfCase {
                name: "FULL SQUASH",
                set: &[("gain", 22.0), ("squash", 16.0), ("focus", 1.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
