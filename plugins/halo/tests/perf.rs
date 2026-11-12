use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_halo_vst3::Halo>(
        "halo",
        &[
            PerfCase {
                name: "OPEN",
                set: &[],
            },
            PerfCase {
                name: "MAXIMUM BLOOM",
                set: &[("bloom", 1.0), ("reserve", 1.0), ("response", 0.0)],
            },
        ],
        &PerfConfig::default(),
    );
}
