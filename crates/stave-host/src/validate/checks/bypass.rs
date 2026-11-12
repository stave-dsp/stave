use serde_json::json;

use crate::param::Param;
use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::compare::null_test;

#[derive(Copy, Clone, Debug)]
pub struct BypassConfig {
    pub crossfade_settle_seconds: f64,
    pub capture_seconds: f64,
}

impl Default for BypassConfig {
    fn default() -> Self {
        Self {
            crossfade_settle_seconds: 0.3,
            capture_seconds: 0.25,
        }
    }
}

pub fn bypass_honest(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.bypass);
    let mut inst = ctx.fresh()?;
    let params = inst.params();
    let Some(bypass) = find_bypass(&params) else {
        return Ok(Outcome::not_applicable("no bypass param"));
    };
    inst.set_param(&bypass.name, bypass.max)?;
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let layout = inst.layout();
    let latency = inst.latency() as u64;
    let mut d = Driver::new(inst.as_mut());
    d.skip(
        d.frames_for(config.crossfade_settle_seconds),
        signal.as_ref(),
        &notes,
    );
    let cap = d.capture(
        d.frames_for(config.capture_seconds),
        signal.as_ref(),
        &notes,
    );

    if layout.is_instrument() {
        let leak = cap.channels.iter().flatten().find(|v| **v != 0.0);
        return Ok(match leak {
            None => Outcome::pass(json!({ "param": bypass.name })),
            Some(v) => Outcome::fail(json!({ "error": format!("bypassed instrument leaks {v}") })),
        });
    }
    for c in 0..layout.inputs.min(layout.outputs) {
        let got = cap.channel(c);
        let want: Vec<f32> = (0..got.len() as u64)
            .map(|i| cap.start + i)
            .map(|t| t.checked_sub(latency).map_or(0.0, |s| signal.at(c, s)))
            .collect();
        if let Some(i) = got
            .iter()
            .zip(&want)
            .position(|(g, w)| g.to_bits() != w.to_bits())
        {
            return Ok(Outcome::fail(json!({
                "error": "bypassed output is not the delayed dry input",
                "channel": c, "frame": i, "got": got[i], "want": want[i],
                "null": null_test(got, &want),
            })));
        }
    }
    Ok(Outcome::pass(
        json!({ "param": bypass.name, "latency_compensated": latency }),
    ))
}

fn find_bypass(params: &[Param]) -> Option<&Param> {
    params.iter().find(|c| c.flags.bypass).or_else(|| {
        params
            .iter()
            .find(|c| c.leaf().eq_ignore_ascii_case("bypass"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::{test_param, ParamId};
    use crate::instance::Instance;
    use crate::testing::{Delay, Gain, Synth};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    #[test]
    fn honest_effect_and_instrument_pass() {
        let mut g = Gain::new(2, 2);
        g.set_param("gain", 3.0).unwrap();
        let v = outcome(bypass_honest, move || Box::new(g.clone()));
        assert_eq!(v.status, Status::Pass, "{:?}", v.details);
        assert_eq!(
            outcome(bypass_honest, || Box::new(Synth::new())).status,
            Status::Pass
        );
    }

    #[test]
    fn leaky_bypass_fails_with_a_null_residual() {
        let v = outcome(bypass_honest, || {
            let mut g = Gain::new(1, 1);
            g.leaky_bypass = true;
            g.set_param("gain", 2.0).unwrap();
            Box::new(g)
        });
        assert_eq!(v.status, Status::Fail);
        let rel = v.details["null"]["residual_rel_db"].as_f64().unwrap();
        assert!(rel.abs() < 0.01, "2× gain leaves a 0 dB residual: {rel}");
    }

    #[test]
    fn no_bypass_is_info() {
        assert_eq!(
            outcome(bypass_honest, || Box::new(Delay::new(1, 1))).status,
            Status::NotApplicable
        );
    }

    #[test]
    fn flag_beats_name() {
        let mut named = test_param("Global · bypass", 0.0, 1.0);
        named.id = ParamId(1);
        let mut flagged = test_param("Power", 0.0, 1.0);
        flagged.flags.bypass = true;
        assert_eq!(
            find_bypass(&[named.clone(), flagged]).unwrap().name,
            "Power"
        );
        assert_eq!(find_bypass(&[named]).unwrap().name, "Global · bypass");
    }
}
