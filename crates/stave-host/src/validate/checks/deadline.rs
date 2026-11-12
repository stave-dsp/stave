use serde_json::json;

use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome, Status};
use stave_measure::stats::Summary;

/// `p99 / budget` below `pass_below` passes, at or above `fail_at` fails.
#[derive(Copy, Clone, Debug)]
pub struct DeadlineConfig {
    pub warmup_seconds: f64,
    pub run_seconds: f64,
    pub pass_below: f64,
    pub fail_at: f64,
}

impl Default for DeadlineConfig {
    fn default() -> Self {
        Self {
            warmup_seconds: 0.25,
            run_seconds: 1.0,
            pass_below: 0.5,
            fail_at: 1.0,
        }
    }
}

impl DeadlineConfig {
    fn classify(&self, fraction_of_budget: f64) -> Status {
        if fraction_of_budget < self.pass_below {
            Status::Pass
        } else if fraction_of_budget < self.fail_at {
            Status::Info
        } else {
            Status::Fail
        }
    }
}

pub fn deadline_headroom(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.deadline);
    let mut inst = ctx.fresh()?;
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let rate = inst.layout().sample_rate;
    let mut d = Driver::new(inst.as_mut());
    d.skip(d.frames_for(config.warmup_seconds), signal.as_ref(), &notes);
    let mut ns = vec![];
    let n = d.frames_for(config.run_seconds);
    d.run(n, signal.as_ref(), &notes, |b| {
        ns.push(b.process_time().as_nanos() as f64)
    });
    let budget = d.block() as f64 / rate * 1e9;
    let p99 = Summary::of(&ns).map_or(0.0, |s| s.p99);
    let frac = p99 / budget;
    // A debug build's timing says nothing about the shipped plugin's, and
    // on a shared CI runner it is mostly the neighbours': there a miss is
    // reported, not failed. Release builds (stave-bench, the perf cases)
    // are held to the budget.
    let status = match config.classify(frac) {
        Status::Fail if cfg!(debug_assertions) => Status::Info,
        status => status,
    };
    let details = json!({
        "p99_ns_per_block": p99,
        "budget_ns": budget,
        "p99_of_budget": frac,
        "debug_build": cfg!(debug_assertions),
    });
    Ok(Outcome { status, details })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Gain;
    use crate::validate::checks::outcome;

    #[test]
    fn thresholds() {
        let config = DeadlineConfig::default();
        assert_eq!(config.classify(0.1), Status::Pass);
        assert_eq!(config.classify(0.5), Status::Info);
        assert_eq!(config.classify(1.0), Status::Fail);
    }

    #[test]
    fn a_trivial_plugin_has_headroom() {
        let v = outcome(deadline_headroom, || Box::new(Gain::new(2, 2)));
        assert_eq!(v.status, Status::Pass, "{:?}", v.details);
    }
}
