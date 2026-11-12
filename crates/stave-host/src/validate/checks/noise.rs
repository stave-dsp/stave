use serde_json::json;

use crate::note::NoteSchedule;
use crate::render::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::level::{db, Meter};
use stave_measure::signal::Silence;

/// Silent input and no notes, after a settling time: the output's peak
/// must stay below `max_dbfs`. A higher floor is hiss, hum, a DC offset
/// or a denormal storm.
#[derive(Copy, Clone, Debug)]
pub struct NoisePolicy {
    pub settle_seconds: f64,
    pub listen_seconds: f64,
    pub max_dbfs: f64,
}

impl Default for NoisePolicy {
    fn default() -> Self {
        Self {
            settle_seconds: 0.1,
            listen_seconds: 0.5,
            max_dbfs: -80.0,
        }
    }
}

pub fn self_noise(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let policy = ctx.policy.noise;
    let mut inst = ctx.fresh()?;
    let quiet = NoteSchedule::default();
    let mut d = Driver::new(inst.as_mut());
    d.skip(d.frames_for(policy.settle_seconds), &Silence, &quiet);
    let mut meter = Meter::default();
    let n = d.frames_for(policy.listen_seconds);
    d.run(n, &Silence, &quiet, |b| meter.add(&b.outputs()));
    let peak_dbfs = db(meter.peak());
    let details = json!({ "peak_dbfs": peak_dbfs, "max_dbfs": policy.max_dbfs });
    Ok(if meter.non_finite() == 0 && peak_dbfs < policy.max_dbfs {
        Outcome::pass(details)
    } else {
        Outcome::fail(details)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{Instance, Layout};
    use crate::note::NoteEvent;
    use crate::testing::Gain;
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    /// Outputs a constant offset whatever comes in.
    struct Offset;

    impl Instance for Offset {
        fn layout(&self) -> Layout {
            crate::testing::layout(1, 1)
        }
        fn process(&mut self, _: &[&[f32]], o: &mut [&mut [f32]], _: &[NoteEvent]) {
            o[0].fill(1e-3);
        }
    }

    #[test]
    fn silence_through_a_gain_is_silent_and_an_offset_is_not() {
        assert_eq!(
            outcome(self_noise, || Box::new(Gain::new(2, 2))).status,
            Status::Pass
        );
        let offset = outcome(self_noise, || Box::new(Offset));
        assert_eq!(offset.status, Status::Fail);
        assert!((offset.details["peak_dbfs"].as_f64().unwrap() + 60.0).abs() < 0.01);
    }
}
