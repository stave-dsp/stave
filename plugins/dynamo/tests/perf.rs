//! The perf report against the realtime budget (stave-test::
//! perf report — documents, never blocks; for instruments it holds an
//! 8-note chord, since an idle rack's cost is not the number that
//! matters). Baselines 2026-07-07 (Apple Silicon, ns/frame):
//! SINE CHORD ~49 (the f64 sin per voice), SAW CHORD ~21 — both
//! ≤ 0.2% of budget.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_dynamo_vst3::Dynamo>(
        "dynamo",
        &[
            PerfCase {
                name: "SINE CHORD",
                set: &[],
            },
            PerfCase {
                name: "PCM WHEEL CHORD",
                set: &[("wheel", 7.0), ("poles", 6.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
