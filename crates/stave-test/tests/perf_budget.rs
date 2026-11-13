//! The perf check holds a plugin to its declared CPU budget (DSP.md P6):
//! over it, the verdict names the case, the searched worst knobs and the
//! stage its cost taps say took the time; undeclared, it measures and
//! never fails. Runs in any build through `perf::measure` (the mounted
//! check skips debug builds), with budgets no build can meet or miss.

use std::hint::black_box;

use stave::prelude::*;
use stave_test::perf::{measure, PerfCase, PerfConfig, WORST_KNOBS};

#[derive(Params)]
struct WorkParams {
    /// 0: one step a sample; 1: 400.
    work: FloatParam,
    /// Costs nothing either way.
    mix: FloatParam,
}

impl Default for WorkParams {
    fn default() -> Self {
        WorkParams {
            work: FloatParam::new(ParamId(1), "Work", Range::linear(0.0, 1.0)),
            mix: FloatParam::new(ParamId(2), "Mix", Range::linear(0.0, 1.0)).default(1.0),
        }
    }
}

/// Burns `1 + 399 × work` steps a sample inside a stage called `burn`.
struct Burner<const DECLARED: bool> {
    params: Arc<WorkParams>,
    cost: CostTaps,
}

impl<const DECLARED: bool> Plugin for Burner<DECLARED> {
    const INFO: Info = Info { id: "com.stave.test.burner", name: "Burner", vendor: "stave tests", ..Info::EFFECT };
    const CPU_BUDGET: Budget = if DECLARED { Budget::per_channel(0.001) } else { Budget::UNDECLARED };
    type Params = WorkParams;

    fn new(params: &Arc<WorkParams>, telemetry: &Telemetry) -> Self {
        Burner { params: params.clone(), cost: CostTaps::new(telemetry, cost_tap_ids!("burn")) }
    }

    fn prepare(&mut self, env: &Environment) {
        self.cost.prepare(env.sample_rate);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let steps = 1 + (399.0 * self.params.work.value()) as u32;
        let frames = block.frames();
        let (input, output) = block.io();
        self.cost.measure(frames, || {
            for (x, y) in input.channels().zip(output.channels_mut()) {
                for (x, y) in x.iter().zip(y) {
                    let mut acc = *x;
                    for _ in 0..steps {
                        acc = black_box(acc * 0.999_9 + 1e-9);
                    }
                    *y = acc;
                }
            }
        });
    }
}

fn quick() -> PerfConfig {
    PerfConfig {
        warmup_blocks: 2,
        repeats: 3,
        seconds_per_repeat: 0.03,
        scan_warmup_blocks: 1,
        scan_seconds: 0.03,
        stage_seconds: 0.03,
        ..PerfConfig::default()
    }
}

#[test]
fn over_budget_names_the_case_the_worst_knobs_and_the_hotspot() {
    let cases = [PerfCase { name: "DEFAULTS", set: &[] }];
    let report = measure::<Burner<true>>("Burner", &cases, &quick());
    assert_eq!(report.channels, 2);
    assert_eq!(report.rows.len(), 2, "the declared case and the worst knobs");
    let worst = report.rows.iter().find(|r| r.name == WORST_KNOBS).unwrap();
    // The search found the knob that costs: 400 steps against 1.
    assert!(worst.set.iter().any(|(n, v)| n == "Work" && (*v - 1.0).abs() < 1e-9), "{:?}", worst.set);
    assert!(worst.ns_per_sample_per_channel > report.rows[0].ns_per_sample_per_channel);
    assert_eq!(worst.stages.first().map(|s| s.name.as_str()), Some("burn"));
    let verdict = report.verdict().expect("0.001 ns a sample is over any machine");
    for needle in ["over its CPU budget", "DEFAULTS", WORST_KNOBS, "Work=1", "hotspot: burn"] {
        assert!(verdict.contains(needle), "{needle:?} missing from:\n{verdict}");
    }
}

#[test]
fn undeclared_reports_and_never_fails() {
    let report = measure::<Burner<false>>("Burner", &[], &PerfConfig { worst_case: false, ..quick() });
    assert!(!report.budget.is_declared());
    assert_eq!(report.rows.len(), 0);
    assert!(report.verdict().is_none());
    let report = measure::<Burner<false>>("Burner", &[PerfCase { name: "HOT", set: &[("work", 1.0)] }], &quick());
    assert!(report.rows.iter().all(|r| r.ns_per_sample_per_channel > 0.0));
    assert!(report.verdict().is_none(), "no budget, no failure");
}
