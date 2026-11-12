use serde_json::json;

use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::level::{db, Meter};

#[derive(Copy, Clone, Debug)]
pub struct SweepConfig {
    /// Values per continuous param (see [`Param::sweep_values`]).
    ///
    /// [`Param::sweep_values`]: crate::param::Param::sweep_values
    pub points: usize,
    pub settle_seconds: f64,
    pub hold_seconds: f64,
    /// Each hold is split into this many windows for the growth test.
    pub windows: usize,
    /// Runaway is growth: the hold's last quarter louder than its first by
    /// more than this under steady excitation. A loud but steady setting
    /// (an output trim) is the user's choice, not instability.
    pub runaway_growth_db: f64,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            points: 5,
            settle_seconds: 0.05,
            hold_seconds: 0.1,
            windows: 8,
            runaway_growth_db: 12.0,
        }
    }
}

pub fn param_sweep(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.sweep);
    let mut inst = ctx.fresh()?;
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let params: Vec<_> = inst
        .params()
        .into_iter()
        .filter(|c| c.sweepable())
        .collect();
    let mut d = Driver::new(inst.as_mut());
    d.skip(d.frames_for(config.settle_seconds), signal.as_ref(), &notes);
    if params.is_empty() {
        return Ok(Outcome::not_applicable("no sweepable params"));
    }
    let mut offenders = vec![];
    let mut loudest = (f64::NEG_INFINITY, json!(null));
    for c in &params {
        for v in c.sweep_values(config.points) {
            d.instance().set_param(&c.name, v)?;
            let n = d.frames_for(config.hold_seconds) / config.windows as u64;
            let windows: Vec<Meter> = (0..config.windows)
                .map(|_| {
                    let mut m = Meter::default();
                    d.run(n, signal.as_ref(), &notes, |b| m.add(&b.outputs().collect::<Vec<_>>()));
                    m
                })
                .collect();
            let non_finite: u64 = windows.iter().map(Meter::non_finite).sum();
            let quarter = (config.windows / 4).max(1);
            let level = |w: &[Meter]| db(w.iter().map(Meter::rms).fold(0.0, f64::max));
            let growth_db =
                level(&windows[config.windows - quarter..]) - level(&windows[..quarter]);
            let peak_dbfs = db(windows.iter().map(Meter::peak).fold(0.0, f64::max));
            if peak_dbfs > loudest.0 {
                loudest = (
                    peak_dbfs,
                    json!({ "param": c.name, "value": v, "peak_dbfs": peak_dbfs }),
                );
            }
            if non_finite > 0 || growth_db > config.runaway_growth_db {
                offenders.push(json!({
                    "param": c.name, "value": v,
                    "non_finite_samples": non_finite, "growth_db": growth_db,
                }));
            }
        }
        d.instance().set_param(&c.name, c.default)?;
        d.skip(d.frames_for(config.settle_seconds), signal.as_ref(), &notes);
    }
    let swept = params.len();
    Ok(if offenders.is_empty() {
        Outcome::pass(json!({ "params_swept": swept, "loudest": loudest.1 }))
    } else {
        Outcome::fail(json!({ "params_swept": swept, "offenders": offenders }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Delay, Feedback, Gain};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    #[test]
    fn sane_ranges_pass_and_count_params() {
        let v = outcome(param_sweep, || Box::new(Gain::new(1, 1)));
        assert_eq!(
            (v.status, v.details["params_swept"].as_u64()),
            (Status::Pass, Some(2))
        );
    }

    #[test]
    fn a_loud_but_steady_setting_is_not_runaway() {
        let v = outcome(param_sweep, || Box::new(Gain::with_max(1, 1, 1000.0)));
        assert_eq!(v.status, Status::Pass, "{}", v.details);
        assert!(v.details["loudest"]["peak_dbfs"].as_f64().unwrap() > 40.0);
    }

    #[test]
    fn growth_is_runaway_and_names_the_param_and_value() {
        let v = outcome(param_sweep, || Box::new(Feedback::new(1)));
        assert_eq!(v.status, Status::Fail);
        let offenders = v.details["offenders"].as_array().unwrap();
        assert!(offenders.iter().all(|o| o["param"] == "feedback"));
        let values: Vec<f64> = offenders
            .iter()
            .map(|o| o["value"].as_f64().unwrap())
            .collect();
        assert!(
            values.iter().all(|&v| v > 1.0),
            "only feedback above 1 grows: {values:?}"
        );
    }

    #[test]
    fn nothing_to_sweep_is_info_not_pass() {
        let v = outcome(param_sweep, || Box::new(Delay::new(1, 1)));
        assert_eq!(v.status, Status::NotApplicable);
    }
}
