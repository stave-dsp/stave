//! The perf report: multiband's representative configurations against
//! the realtime budget (documents, never blocks — see stave_test::perf).
//! Baseline 2026-10-13 (Apple Silicon, ns/frame): DEFAULT ~120, DRIVEN ~770
//! (saturators engaged), OPTO ~150.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_multiband_comp_vst3::MultibandComp>(
        "multiband",
        &[
            PerfCase {
                name: "DEFAULT",
                set: &[],
            },
            PerfCase {
                name: "DRIVEN",
                set: &[("lo drive", 12.0), ("mid drive", 12.0), ("hi drive", 12.0)],
            },
            PerfCase {
                name: "OPTO",
                set: &[("lo style", 2.0), ("mid style", 2.0), ("hi style", 2.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
