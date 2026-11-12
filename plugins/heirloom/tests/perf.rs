//! The perf report: each machine at full loops with the record worn
//! hard, plus the worst clock case (BLOOM stretched fast while HOLD
//! sags it). It documents, it does not block (stave_test::perf).

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_heirloom_vst3::Heirloom>(
        "heirloom",
        &[
            PerfCase { name: "REVERSE worn", set: &[("machine", 0.0), ("repeats", 0.8), ("age", 0.8), ("wobble", 0.6)] },
            PerfCase { name: "OCTAVE worn", set: &[("machine", 1.0), ("repeats", 0.8), ("age", 0.8), ("media", 1.0)] },
            PerfCase { name: "BLOOM worn", set: &[("machine", 2.0), ("repeats", 0.8), ("age", 0.8), ("media", 2.0)] },
            PerfCase { name: "BLOOM fast + held", set: &[("machine", 2.0), ("time", 50.0), ("hold", 1.0)] },
        ],
        &PerfConfig::default(),
    );
}
