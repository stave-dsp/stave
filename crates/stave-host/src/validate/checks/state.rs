use serde_json::json;

use crate::instance::StateError;
use crate::render::Driver;
use crate::validate::{CheckError, Ctx, Outcome};

/// Controls are parked at `nudge` through their range, far from any
/// default, so a state that ignores them shows.
#[derive(Copy, Clone, Debug)]
pub struct StatePolicy {
    pub nudge: f64,
    pub settle_seconds: f64,
}

impl Default for StatePolicy {
    fn default() -> Self {
        Self {
            nudge: 0.618,
            settle_seconds: 0.1,
        }
    }
}

pub fn state_roundtrip(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, policy) = (&ctx.policy.excitation, ctx.policy.state);
    let (mut a, mut b) = (ctx.fresh()?, ctx.fresh()?);
    let (signal, notes) = excitation.for_instance(a.as_ref());
    let nudged: Vec<_> = a
        .controls()
        .into_iter()
        .filter(|c| c.sweepable() && !c.flags.bypass)
        .filter(|c| a.set_control(&c.name, c.lerp(policy.nudge)).is_ok())
        .map(|c| c.name)
        .collect();
    let mut d = Driver::new(a.as_mut());
    d.skip(d.frames_for(policy.settle_seconds), signal.as_ref(), &notes);
    let blob = match a.save_state() {
        Ok(blob) => blob,
        Err(StateError::Unsupported) => {
            return Ok(Outcome::not_applicable("instance has no state"))
        }
        Err(e) => return Ok(Outcome::fail(json!({ "error": e.to_string() }))),
    };
    if let Err(e) = b.load_state(&blob) {
        return Ok(Outcome::fail(
            json!({ "error": format!("own blob refused: {e}") }),
        ));
    }
    let again = b.save_state()?;
    Ok(if again == blob {
        Outcome::pass(json!({ "bytes": blob.len(), "controls_nudged": nudged.len() }))
    } else {
        let at = blob.iter().zip(&again).position(|(x, y)| x != y);
        Outcome::fail(json!({
            "error": "state changed through the round-trip",
            "first_diff_byte": at.unwrap_or(blob.len().min(again.len())),
            "bytes": [blob.len(), again.len()],
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{Instance, Layout};
    use crate::note::NoteEvent;
    use crate::testing::{Delay, Gain};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    #[test]
    fn honest_state_passes_with_controls_nudged() {
        let v = outcome(state_roundtrip, || Box::new(Gain::new(1, 1)));
        assert_eq!(v.status, Status::Pass, "{:?}", v.details);
        assert_eq!(v.details["controls_nudged"], 1, "bypass is left alone");
    }

    #[test]
    fn stateless_instance_is_info() {
        assert_eq!(
            outcome(state_roundtrip, || Box::new(Delay::new(1, 1))).status,
            Status::NotApplicable
        );
    }

    struct Liar(u8, Layout);
    impl Instance for Liar {
        fn layout(&self) -> Layout {
            self.1
        }
        fn process(&mut self, _: &[&[f32]], o: &mut [&mut [f32]], _: &[NoteEvent]) {
            o.iter_mut().for_each(|c| c.fill(0.0));
        }
        fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
            self.0 += 1;
            Ok(vec![self.0])
        }
        fn load_state(&mut self, b: &[u8]) -> Result<(), StateError> {
            self.0 = b[0];
            Ok(())
        }
    }

    #[test]
    fn mutating_state_fails_with_the_first_diff() {
        let v = outcome(state_roundtrip, || {
            Box::new(Liar(0, crate::testing::layout(1, 1)))
        });
        assert_eq!(v.status, Status::Fail);
        assert_eq!(v.details["first_diff_byte"], 0);
    }
}
