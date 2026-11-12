use serde_json::json;

use crate::instance::{Instance, TapReading};
use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};

/// Every sweepable param is held at each end of its range while the
/// stimulus plays, since solvers fail at the extremes, not the defaults.
#[derive(Copy, Clone, Debug)]
pub struct SolverConfig {
    pub hold_seconds: f64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self { hold_seconds: 0.25 }
    }
}

pub fn solver_converges(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.solver);
    let mut inst = ctx.fresh()?;
    if read(inst.as_ref(), TapReading::NEWTON_SOLVES).is_none() {
        return Ok(Outcome::not_applicable("no solver telemetry"));
    }
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let params: Vec<_> = inst
        .params()
        .into_iter()
        .filter(|c| c.sweepable())
        .collect();
    let mut d = Driver::new(inst.as_mut());
    let hold = d.frames_for(config.hold_seconds);
    d.skip(hold, signal.as_ref(), &notes);
    for c in &params {
        for v in [c.min, c.max, c.default] {
            d.instance().set_param(&c.name, v)?;
            d.skip(hold, signal.as_ref(), &notes);
        }
    }
    let inst = d.instance();
    let solves = read(inst, TapReading::NEWTON_SOLVES).unwrap_or(0.0);
    let capped = read(inst, TapReading::NEWTON_CAPPED).unwrap_or(0.0);
    let details = json!({
        "solves": solves,
        "capped": capped,
        "mean_iterations": read(inst, TapReading::NEWTON_MEAN_ITERATIONS),
        "max_final_step": read(inst, TapReading::NEWTON_MAX_FINAL_STEP),
        "params_swept": params.len(),
    });
    Ok(if capped == 0.0 {
        Outcome::pass(details)
    } else {
        Outcome::fail(details)
    })
}

fn read(inst: &dyn Instance, name: &str) -> Option<f64> {
    inst.taps()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.latest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Layout;
    use stave_core::Event;
    use crate::testing::Gain;
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    /// Reports a solver that caps once every `every` solves.
    struct Stalls {
        every: u64,
        solves: u64,
    }

    impl Instance for Stalls {
        fn layout(&self) -> Layout {
            crate::testing::layout(1, 1)
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[Event]) {
            o[0].copy_from_slice(i[0]);
            self.solves += i[0].len() as u64;
        }
        fn taps(&self) -> Vec<TapReading> {
            let tap = |name: &str, v: f64| TapReading {
                name: name.into(),
                latest: v,
                min: v,
                max: v,
            };
            let capped = self.solves.checked_div(self.every).unwrap_or(0);
            vec![
                tap(TapReading::NEWTON_SOLVES, self.solves as f64),
                tap(TapReading::NEWTON_CAPPED, capped as f64),
            ]
        }
    }

    #[test]
    fn converging_passes_capping_fails_and_no_solver_is_info() {
        let ok = outcome(solver_converges, || {
            Box::new(Stalls {
                every: 0,
                solves: 0,
            })
        });
        assert_eq!(ok.status, Status::Pass, "{:?}", ok.details);
        let bad = outcome(solver_converges, || {
            Box::new(Stalls {
                every: 1000,
                solves: 0,
            })
        });
        assert_eq!(bad.status, Status::Fail);
        assert!(bad.details["capped"].as_f64().unwrap() > 0.0);
        assert_eq!(
            outcome(solver_converges, || Box::new(Gain::new(1, 1))).status,
            Status::NotApplicable
        );
    }
}
