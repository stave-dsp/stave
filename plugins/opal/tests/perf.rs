//! The perf report against the realtime budget (instruments hold an
//! 8-note chord). Baselines 2026-07-07 (Apple Silicon, ns/frame):
//! INIT CHORD ~510, FULL STACK (all four ops + feedback) ~637 —
//! ~3% of budget for 8 voices × 4 operators × 4 envelopes, all
//! per-sample f64 sin. Headroom for the whole tier-3 wishlist.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_opal_vst3::Opal>(
        "opal",
        &[
            PerfCase {
                name: "INIT CHORD",
                set: &[],
            },
            PerfCase {
                name: "FULL STACK",
                set: &[
                    ("algo", 0.0),
                    ("level 2", 1.0),
                    ("level 3", 1.0),
                    ("level 4", 1.0),
                    ("feedback", 0.9),
                ],
            },
        ],
        &PerfConfig::default(),
    );
}
