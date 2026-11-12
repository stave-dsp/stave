use serde_json::json;

use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::level::{db, peak, rms};
use stave_measure::signal::db_to_amp;

/// A host move of a continuous control should glide, not step. Two
/// identical runs differ only by one move of the control between
/// blocks; their difference is the control's own effect. A glide starts
/// that effect near zero, a step starts it at full size (a click).
#[derive(Copy, Clone, Debug)]
pub struct GlideConfig {
    pub settle_seconds: f64,
    pub window_seconds: f64,
    /// The move, as positions across the control's range.
    pub from: f64,
    pub to: f64,
    /// The effect has appeared once it reaches this, relative to its
    /// settled peak.
    pub appear_db: f64,
    /// Samples from its appearance that count as the effect's onset: a
    /// click is a discontinuity, so a short span.
    pub onset_samples: usize,
    /// The largest onset of the effect, relative to its settled peak.
    pub max_onset_db: f64,
    /// Effects quieter than this, relative to the output, are not
    /// measured.
    pub min_effect_db: f64,
}

impl Default for GlideConfig {
    fn default() -> Self {
        Self {
            settle_seconds: 0.2,
            window_seconds: 0.1,
            from: 0.4,
            to: 0.6,
            appear_db: -60.0,
            onset_samples: 2,
            max_onset_db: -20.0,
            min_effect_db: -80.0,
        }
    }
}

pub fn controls_glide(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.glide);
    let instance = ctx.fresh()?;
    let controls: Vec<_> = instance
        .controls()
        .into_iter()
        .filter(|c| c.flags.automatable && c.sweepable() && c.steps == 0 && !c.flags.bypass)
        .collect();
    if controls.is_empty() {
        return Ok(Outcome::not_applicable(
            "no automatable continuous controls",
        ));
    }
    let mut steps = vec![];
    let mut measured = 0;
    for c in &controls {
        let mut runs = vec![];
        for moved in [false, true] {
            let mut inst = ctx.fresh()?;
            let (signal, notes) = excitation.for_instance(inst.as_ref());
            inst.set_control(&c.name, c.lerp(config.from))?;
            let mut d = Driver::new(inst.as_mut());
            d.skip(d.frames_for(config.settle_seconds), signal.as_ref(), &notes);
            if moved {
                d.instance().set_control(&c.name, c.lerp(config.to))?;
            }
            let n = d.frames_for(config.window_seconds);
            runs.push(d.capture(n, signal.as_ref(), &notes).channel(0).to_vec());
        }
        let (held, moved) = (&runs[0], &runs[1]);
        let effect: Vec<f32> = moved.iter().zip(held).map(|(m, h)| m - h).collect();
        let settled = peak(&effect[effect.len() / 2..]);
        if db(settled) - db(rms(held)) < config.min_effect_db {
            continue;
        }
        measured += 1;
        let floor = (settled * db_to_amp(config.appear_db)) as f32;
        let appears = effect.iter().position(|e| e.abs() >= floor).unwrap_or(0);
        let onset = &effect[appears..(appears + config.onset_samples).min(effect.len())];
        let onset_db = db(peak(onset)) - db(settled);
        if onset_db > config.max_onset_db {
            steps.push(json!({ "control": c.name, "onset_db": onset_db }));
        }
    }
    let details = json!({ "measured": measured, "steps": steps });
    Ok(if steps.is_empty() {
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

    /// A gain applied as written, with no glide.
    struct Stepped {
        gain: f64,
    }

    impl Instance for Stepped {
        fn layout(&self) -> Layout {
            crate::testing::layout(2, 2)
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[NoteEvent]) {
            for (out, inp) in o.iter_mut().zip(i) {
                for (y, x) in out.iter_mut().zip(*inp) {
                    *y = x * self.gain as f32;
                }
            }
        }
        fn controls(&self) -> Vec<crate::control::Control> {
            Gain::new(2, 2).controls()
        }
        fn set_control(&mut self, _: &str, plain: f64) -> Result<(), crate::control::ControlError> {
            self.gain = plain;
            Ok(())
        }
    }

    #[test]
    fn a_stepped_control_fails_and_a_gliding_one_passes() {
        let step = outcome(controls_glide, || Box::new(Stepped { gain: 1.0 }));
        assert_eq!(step.status, Status::Fail, "{:?}", step.details);
        let glide = outcome(controls_glide, || Box::new(Gain::new(2, 2)));
        assert_eq!(glide.status, Status::Pass, "{:?}", glide.details);
    }
}
