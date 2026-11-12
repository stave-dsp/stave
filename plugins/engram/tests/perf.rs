//! The perf report: representative configurations measured against
//! the realtime budget (stave_test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_engram_vst3::Engram>(
        "engram",
        &[
            PerfCase { name: "DEFAULT", set: &[] },
            PerfCase { name: "big, long, soft focus", set: &[("size", 1.0), ("decay", 30.0), ("focus", 0.0), ("deja", 0.0), ("recall", 1.0)] },
            PerfCase { name: "held", set: &[("hold", 1.0)] },
        ],
        &PerfConfig::default(),
    );
}
