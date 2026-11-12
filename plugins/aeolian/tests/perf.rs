//! The perf report: representative configurations measured against
//! the realtime budget (stave_test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_aeolian_vst3::Aeolian>(
        "aeolian",
        &[
            PerfCase { name: "DEFAULT", set: &[] },
            PerfCase { name: "everything on", set: &[("sustain", 1.0), ("wind", 1.0), ("sway", 1.0), ("air", 1.0), ("drift", 1.0)] },
            PerfCase { name: "HIGH register", set: &[("register", 4.0), ("sustain", 0.9)] },
        ],
        &PerfConfig::default(),
    );
}
