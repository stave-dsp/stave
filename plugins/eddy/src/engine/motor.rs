//! MOTOR — the transport is a physical machine: program energy loads
//! the motor, the platter slows, the delay time (and pitch) sags flat,
//! then recovers as the tail decays. Program-dependent wow, in the
//! tape-echo lineage — but here the LOAD is the wet tail itself, so
//! the machine reacts to its own echoes.
//!
//! SAG is how much the motor cares about load; INERTIA is the
//! platter's mass — how slowly the sag leans in and lets go. The
//! tape path itself is FIXED — one machine, one head: a moderate
//! magnetization + slew + generation-loss filter at the write head
//! (the knobs steer the motor, not the tape). Small-signal unity and
//! cut-only, so the loop stays caged at the dialed feedback.

use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, EnvelopeFollower, EnvelopeTimes, Lag, Slew, Stage};

use super::diffusion::Diffusion;
use super::echo::Echo;

/// Full-load time stretch at SAG = 1: the platter loses a quarter of
/// its speed. Sized into the delay-line capacity (mod.rs).
pub(super) const SAG_MAX: f32 = 0.25;

/// One lane of the machine's tape path (see the header): moderate
/// fixed drive — texture, not distortion.
struct LaneTape {
    mag: stave_dsp::Hysteresis,
    slew: Slew,
    loss: Biquad,
}

pub(super) struct Motor {
    /// How loud the tail is right now — the motor's load.
    load: EnvelopeFollower,
    /// The platter's mass: load reaches the speed through this.
    inertia: Lag,
    /// The fixed tape path, per lane.
    tape: stave_dsp::PerChannel<LaneTape, 2>,
    /// Last frame's peak INTO the head (program + fed-back tail —
    /// the flux the machine is asked to record; the tape's own
    /// ceiling must not hide load from the motor). One frame late —
    /// the same explicit-delay rule every feedback obeys.
    last_head: f32,
    sr: f32,
}

impl Motor {
    pub(super) fn new() -> Self {
        Self {
            load: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 10.0,
                release_ms: 250.0,
                slow_release_ms: None,
            }),
            inertia: Lag::new(300.0, 300.0),
            tape: stave_dsp::PerChannel::new(|_| LaneTape {
                mag: stave_dsp::Hysteresis::new(2.2, 0.5),
                slew: Slew::new(120.0, 120.0),
                loss: Biquad::new(),
            }),
            last_head: 0.0,
            sr: 48_000.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        // The follower believes fast and forgets at tail speed.
        self.load.prepare(sr);
        self.inertia.prepare(sr);
        self.inertia.reset();
        for lane in self.tape.iter_mut() {
            lane.mag.prepare(sr);
            lane.slew.prepare(sr);
            lane.slew.set_rates(120.0, 120.0, sr);
            lane.loss.set(
                FilterSpec::Lowpass {
                    hz: 6_500.0,
                    q: 0.707,
                },
                sr,
            );
        }
        self.last_head = 0.0;
    }

    /// Block-rate: INERTIA maps to the platter time constant.
    pub(super) fn begin_block(&mut self, inertia: f32) {
        let ms = 60.0 + 1_500.0 * inertia.clamp(0.0, 1.0);
        self.inertia.set_times(ms, ms, self.sr);
    }

    /// One stereo frame: the sagged time through the classic law.
    #[inline]
    pub(super) fn frame(
        &mut self,
        echo: &mut Echo,
        diffusion: &mut Diffusion,
        cx: super::FrameCtx,
        sag: f32,
    ) -> (f32, f32) {
        let load = self.load.tick(self.last_head);
        let droop = self.inertia.tick((load * sag).min(1.0));
        let sagged = cx.base * (1.0 + SAG_MAX * droop);
        let tape = &mut self.tape;
        let mut head_peak = 0.0f32;
        let (wl, wr) = echo.stereo_frame_shaped(
            cx.l,
            cx.r,
            sagged,
            cx.wobble,
            cx.spread,
            cx.fb,
            |lane, x| {
                head_peak = head_peak.max(x.abs());
                let t = &mut tape[lane];
                t.loss.process(t.slew.tick(t.mag.tick(x)))
            },
            |m, s| diffusion.couple(m, s, cx.diffuse, cx.bleed),
        );
        self.last_head = head_peak;
        (wl, wr)
    }
}
