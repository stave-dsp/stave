use serde_json::json;

use crate::drive::Driver;
use crate::validate::{CheckError, Ctx, Outcome};
use stave_measure::level::{db, rms};
use stave_measure::signal::{db_to_amp, Mono, Signal};
use stave_measure::stereo::StereoAnalysis;

/// Identical input on every channel should give identical output on
/// every channel, unless the processor means to differ (a widener, a
/// ping-pong delay); a difference is reported as `info` with the image.
#[derive(Copy, Clone, Debug)]
pub struct ChannelsConfig {
    pub render_seconds: f64,
    /// How long channels run apart after the split.
    pub split_seconds: f64,
    /// The extra level on the second input at the split, relative to the
    /// first.
    pub split_difference_db: f64,
    /// The largest output difference after the split, relative to the
    /// output level. A lane that wakes from rest at the split reads near
    /// 0 dB; a slight input difference stays far below this.
    pub max_split_difference_db: f64,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            render_seconds: 0.5,
            split_seconds: 0.1,
            split_difference_db: -60.0,
            max_split_difference_db: -20.0,
        }
    }
}

/// The excitation on every channel, with channel 1 scaled by
/// `1 + difference` from frame `at`.
struct Split<S> {
    signal: S,
    at: u64,
    difference: f32,
}

impl<S: Signal> Signal for Split<S> {
    fn at(&self, channel: usize, frame: u64) -> f32 {
        let x = self.signal.at(0, frame);
        if channel > 0 && frame >= self.at {
            x * (1.0 + self.difference)
        } else {
            x
        }
    }
}

pub fn channels_linked(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.channels);
    let mut inst = ctx.fresh()?;
    if inst.layout().outputs < 2 {
        return Ok(Outcome::not_applicable("fewer than two outputs"));
    }
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let mono = Mono(signal);
    let mut d = Driver::new(inst.as_mut());
    let n = d.frames_for(config.render_seconds);
    let cap = d.capture(n, &mono, &notes);
    let first = cap.channel(0);
    let differing: Vec<usize> = (1..cap.channels.len())
        .filter(|&c| {
            cap.channel(c)
                .iter()
                .zip(first)
                .any(|(a, b)| a.to_bits() != b.to_bits())
        })
        .collect();
    if differing.is_empty() {
        return Ok(Outcome::pass(json!({ "channels": cap.channels.len() })));
    }
    let other = cap.channel(differing[0]);
    let largest_difference = first
        .iter()
        .zip(other)
        .map(|(a, b)| f64::from((a - b).abs()))
        .fold(0.0, f64::max);
    Ok(Outcome::info(json!({
        "note": "channels differ on identical input",
        "differing_channels": differing,
        "largest_difference_dbfs": db(largest_difference),
        "image": StereoAnalysis::default().analyze(first, other),
    })))
}

/// Linked channels whose inputs then differ slightly must stay close:
/// a processor that runs one lane for both while the inputs match, and
/// starts the second only when they diverge, wakes it from rest.
pub fn channels_split(ctx: &mut Ctx) -> Result<Outcome, CheckError> {
    let (excitation, config) = (&ctx.config.excitation, ctx.config.channels);
    let mut inst = ctx.fresh()?;
    let layout = inst.layout();
    if layout.inputs < 2 || layout.outputs < 2 {
        return Ok(Outcome::not_applicable("fewer than two inputs or outputs"));
    }
    let (signal, notes) = excitation.for_instance(inst.as_ref());
    let mut d = Driver::new(inst.as_mut());
    let split = Split {
        signal,
        at: d.frames_for(config.render_seconds),
        difference: db_to_amp(config.split_difference_db) as f32,
    };
    let before = d.capture(split.at, &split, &notes);
    if before.channel(0) != before.channel(1) {
        return Ok(Outcome::not_applicable(
            "channels differ on identical input",
        ));
    }
    let after = d.capture(d.frames_for(config.split_seconds), &split, &notes);
    let (left, right) = (after.channel(0), after.channel(1));
    let difference: Vec<f32> = left.iter().zip(right).map(|(l, r)| r - l).collect();
    let difference_db = db(rms(&difference)) - db(rms(left));
    let details = json!({ "split_difference_db": difference_db });
    Ok(if difference_db > config.max_split_difference_db {
        Outcome::fail(details)
    } else {
        Outcome::pass(details)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{Instance, Layout};
    use stave_core::Event;
    use crate::testing::{Gain, Synth};
    use crate::validate::checks::outcome;
    use crate::validate::Status;

    /// Inverts the right channel.
    struct Widener;

    impl Instance for Widener {
        fn layout(&self) -> Layout {
            crate::testing::layout(2, 2)
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[Event]) {
            o[0].copy_from_slice(i[0]);
            for (y, x) in o[1].iter_mut().zip(i[1]) {
                *y = -x;
            }
        }
    }

    /// A one-pole lowpass per lane that runs lane 0 alone while the
    /// inputs match, and lane 1 only once they differ.
    #[derive(Default)]
    struct Shortcut {
        state: [f32; 2],
        split: bool,
    }

    impl Instance for Shortcut {
        fn layout(&self) -> Layout {
            crate::testing::layout(2, 2)
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[Event]) {
            for n in 0..i[0].len() {
                self.split |= i[0][n] != i[1][n];
                let lanes = if self.split { 2 } else { 1 };
                for c in 0..lanes {
                    self.state[c] += 0.001 * (i[c][n] - self.state[c]);
                }
                o[0][n] = self.state[0];
                o[1][n] = self.state[if self.split { 1 } else { 0 }];
            }
        }
    }

    #[test]
    fn a_lane_waking_at_the_split_fails_and_linked_lanes_pass() {
        let gain = outcome(channels_split, || Box::new(Gain::new(2, 2)));
        assert_eq!(gain.status, Status::Pass, "{:?}", gain.details);
        let shortcut = outcome(channels_split, || Box::new(Shortcut::default()));
        assert_eq!(shortcut.status, Status::Fail, "{:?}", shortcut.details);
    }

    #[test]
    fn linked_processors_pass_and_wideners_are_reported() {
        for v in [
            outcome(channels_linked, || Box::new(Gain::new(2, 2))),
            outcome(channels_linked, || Box::new(Synth::new())),
        ] {
            assert_eq!(v.status, Status::Pass, "{:?}", v.details);
        }
        let w = outcome(channels_linked, || Box::new(Widener));
        assert_eq!(w.status, Status::Info);
        assert_eq!(w.details["image"]["correlation"], -1.0);
    }
}
