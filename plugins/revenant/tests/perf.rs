//! The perf report: representative configurations measured against
//! the realtime budget (stave_test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_revenant_vst3::Revenant>(
        "revenant",
        &[
            PerfCase { name: "DEFAULT", set: &[] },
            PerfCase { name: "learning hard, long horizon", set: &[("learn", 1.0), ("horizon", 25.0), ("chaos", 0.8)] },
            PerfCase { name: "asleep, dreaming", set: &[("sleep", 1.0), ("dream", 1.0)] },
        ],
        &PerfConfig::default(),
    );
}
