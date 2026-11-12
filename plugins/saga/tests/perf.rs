//! The perf report: each program at full loops, plus the worst
//! clock case (program IV stretched fast AND risen 2× — the bridge
//! runs the most core ticks per host frame). It documents, it does
//! not block (stave_test::perf). Baselines 2026-10-25, Apple
//! Silicon, ns/frame @48k: I 238, II 216, III 226, IV 258, V 154,
//! IV fastest clock 430 (~1–2% of budget).

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_saga_vst3::Saga>(
        "saga",
        &[
            PerfCase { name: "I rev→rev", set: &[("mode", 0.0), ("feedback", 0.8), ("regen", 0.8)] },
            PerfCase { name: "II rev→oct-up", set: &[("mode", 1.0), ("feedback", 0.8), ("regen", 0.8)] },
            PerfCase { name: "III rev→oct-down", set: &[("mode", 2.0), ("feedback", 0.8), ("regen", 0.8)] },
            PerfCase { name: "IV rev-verb→verb", set: &[("mode", 3.0), ("feedback", 0.8), ("regen", 0.8)] },
            PerfCase { name: "V pitch→pitch", set: &[("mode", 4.0), ("feedback", 0.8), ("regen", 0.8)] },
            PerfCase {
                name: "IV fastest clock",
                set: &[("mode", 3.0), ("time", 50.0), ("dive", 1.0), ("shift", 2.0), ("ramp", 50.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
