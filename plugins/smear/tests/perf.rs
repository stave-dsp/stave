//! The perf report: smear's representative configurations against the
//! realtime budget (documents, never blocks — see stave_test::perf).
//! Baseline 2026-10-13 (Apple Silicon, ns/frame): DEFAULT ~810,
//! FULL BLOOM ~950.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_smear_vst3::Smear>(
        "smear",
        &[
            PerfCase {
                name: "DEFAULT",
                set: &[],
            },
            PerfCase {
                name: "FULL BLOOM",
                set: &[
                    ("smear", 1.0),
                    ("bloom", 1.0),
                    ("flux", 1.0),
                    ("shimmer", 1.0),
                ],
            },
        ],
        &PerfConfig::default(),
    );
}
