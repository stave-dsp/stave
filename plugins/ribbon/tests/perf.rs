//! The perf report: representative configurations measured against
//! the realtime budget (stave_test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_ribbon_vst3::Ribbon>(
        "ribbon",
        &[
            PerfCase { name: "DEFAULT", set: &[] },
            PerfCase { name: "worn + driven + runaway", set: &[("wear", 1.0), ("drive", 1.0), ("intensity", 1.0), ("spring", 1.0)] },
            PerfCase { name: "fast tape, erase off", set: &[("rate", 1.6), ("erase", 0.0)] },
        ],
        &PerfConfig::default(),
    );
}
