use serde_json::json;

use crate::render::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::compare::first_divergence;
use stave_measure::level::{db, Meter};

#[derive(Copy, Clone, Debug)]
pub struct OutputPolicy {
    pub render_seconds: f64,
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self {
            render_seconds: 2.0,
        }
    }
}

pub fn reproducible(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let policy = ctx.policy;
    let (mut a, mut b) = (ctx.fresh()?, ctx.fresh()?);
    let render = |inst: &mut dyn crate::instance::Instance| {
        let (signal, notes) = policy.excitation.for_instance(inst);
        let mut d = Driver::new(inst);
        let n = d.frames_for(policy.output.render_seconds);
        d.capture(n, signal.as_ref(), &notes).channels
    };
    let (ra, rb) = (render(a.as_mut()), render(b.as_mut()));
    Ok(match first_divergence(&ra, &rb) {
        None => Outcome::pass(json!({ "frames_compared": ra.first().map_or(0, Vec::len) })),
        Some(d) => Outcome::fail(json!({ "first_divergence": d })),
    })
}

pub fn no_nan_inf(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let policy = ctx.policy;
    let mut inst = ctx.fresh()?;
    let (signal, notes) = policy.excitation.for_instance(inst.as_ref());
    let mut d = Driver::new(inst.as_mut());
    let mut meter = Meter::default();
    let n = d.frames_for(policy.output.render_seconds);
    d.run(n, signal.as_ref(), &notes, |b| meter.add(&b.outputs()));
    Ok(match meter.non_finite() {
        0 => Outcome::pass(json!({ "peak_dbfs": db(meter.peak()) })),
        bad => Outcome::fail(json!({ "non_finite_samples": bad })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Drift, Gain, NanAfter, Synth};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    #[test]
    fn deterministic_plugins_reproduce() {
        assert_eq!(
            outcome(reproducible, || Box::new(Gain::new(2, 2))).status,
            Status::Pass
        );
        assert_eq!(
            outcome(reproducible, || Box::new(Synth::new())).status,
            Status::Pass
        );
    }

    #[test]
    fn per_instance_drift_is_caught_at_frame_zero() {
        let v = outcome(reproducible, || Box::new(Drift::new()));
        assert_eq!(v.status, Status::Fail);
        assert_eq!(v.details["first_divergence"]["frame"], 0);
    }

    #[test]
    fn nan_is_counted() {
        let v = outcome(no_nan_inf, || Box::new(NanAfter::new(1000)));
        assert_eq!(v.status, Status::Fail);
        assert_eq!(v.details["non_finite_samples"], 96_000 - 1000);
        assert_eq!(
            outcome(no_nan_inf, || Box::new(Gain::new(1, 1))).status,
            Status::Pass
        );
    }
}
