use serde_json::json;

use crate::instance::Tail;
use crate::note::{Note, NoteSchedule};
use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::level::{mean_square, power_db};
use stave_measure::signal::Silence;

/// A never-quiet output is `info`, not `fail`: a long tail or
/// self-noise is a judgment call.
#[derive(Copy, Clone, Debug)]
pub struct TailConfig {
    pub burst_seconds: f64,
    pub window_seconds: f64,
    pub quiet_dbfs: f64,
    pub margin_seconds: f64,
    pub infinite_tail_seconds: f64,
}

impl Default for TailConfig {
    fn default() -> Self {
        Self {
            burst_seconds: 0.5,
            window_seconds: 0.25,
            quiet_dbfs: -60.0,
            margin_seconds: 2.0,
            infinite_tail_seconds: 8.0,
        }
    }
}

pub fn silence_tail(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.tail);
    let mut inst = ctx.fresh()?;
    let (signal, _) = excitation.for_instance(inst.as_ref());
    let layout = inst.layout();
    let rate = layout.sample_rate;
    let allowed = match inst.tail() {
        Tail::Samples(n) => {
            (f64::from(n) + f64::from(inst.latency())) / rate + config.margin_seconds
        }
        Tail::Infinite => config.infinite_tail_seconds,
    };
    let notes = if layout.is_instrument() {
        let chord: Vec<Note> = excitation
            .instrument_chord
            .iter()
            .map(|&pitch| Note {
                pitch,
                velocity: excitation.velocity,
                on_s: 0.0,
                off_s: config.burst_seconds,
            })
            .collect();
        NoteSchedule::new(&chord, rate)
    } else {
        NoteSchedule::default()
    };
    let mut d = Driver::new(inst.as_mut());
    d.skip(d.frames_for(config.burst_seconds), signal.as_ref(), &notes);
    let (mut t, mut last_db) = (0.0, f64::INFINITY);
    while t < allowed {
        let w = d.capture(d.frames_for(config.window_seconds), &Silence, &notes);
        last_db = w
            .channels
            .iter()
            .map(|c| power_db(mean_square(c)))
            .fold(f64::MIN, f64::max);
        t += config.window_seconds;
        if last_db < config.quiet_dbfs {
            return Ok(Outcome::pass(
                json!({ "quiet_after_s": t, "allowed_s": allowed }),
            ));
        }
    }
    Ok(Outcome::info(json!({
        "note": "still above the quiet level at the end of the allowed tail",
        "quiet_dbfs": config.quiet_dbfs,
        "allowed_s": allowed,
        "last_rms_dbfs": last_db,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Delay, Gain, Synth};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    #[test]
    fn effects_and_instruments_go_quiet() {
        let gain = outcome(silence_tail, || Box::new(Gain::new(2, 2)));
        let synth = outcome(silence_tail, || Box::new(Synth::new()));
        for v in [&gain, &synth] {
            assert_eq!(v.status, Status::Pass, "{:?}", v.details);
            assert_eq!(
                v.details["quiet_after_s"],
                TailConfig::default().window_seconds
            );
        }
    }

    #[test]
    fn a_delay_tail_takes_as_long_as_the_delay() {
        // 0.3 s of delayed noise plays on after the input stops, so the
        // first fully quiet 0.25 s window is the third: [0.5, 0.75).
        let v = outcome(silence_tail, || Box::new(Delay::new(1, 14_400)));
        assert_eq!(v.status, Status::Pass, "{:?}", v.details);
        assert_eq!(v.details["quiet_after_s"], 0.75);
    }
}
