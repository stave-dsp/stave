//! `Motion` — everything that moves the core CLOCK, per host frame:
//!
//! * the BASE: 1× — except program IV, where TIME is a time-stretch
//!   of the space and sets the clock itself (long = slow = larger);
//! * DIVE/RISE: while held, the clock glides (in octaves, over RAMP)
//!   to SHIFT's absolute target, and back on release. Absolute, as on
//!   the pedal: in IV, a SHIFT equal to TIME's clock does nothing;
//! * MOD: the LFO (sine / square / smoothed random) wobbling the
//!   clock — the whole wet image detunes together, tape-like.

use stave_dsp::Lag;

/// Program IV's TIME → clock span: the knob's travel maps 2× … 0.5×.
const IV_OCTAVES: f32 = 1.0;
/// MOD fully up wobbles the clock this many octaves either way.
const MOD_OCTAVES: f32 = 0.05;

pub(super) struct Motion {
    host_hz: f32,
    /// DIVE progress 0 (base) … 1 (at SHIFT).
    dive: f32,
    phase: f32,
    held: f32,
    target: f32,
    rng: u32,
    /// Rounds the square's corners and the random steps.
    lag: Lag,
}

impl Motion {
    pub(super) fn new() -> Self {
        Self {
            host_hz: 48_000.0,
            dive: 0.0,
            phase: 0.0,
            held: 0.0,
            target: 0.0,
            rng: 0x2545_F491,
            lag: Lag::new(3.0, 3.0),
        }
    }

    pub(super) fn prepare(&mut self, host_hz: f32) {
        self.host_hz = host_hz;
        self.dive = 0.0;
        self.phase = 0.0;
        self.held = 0.0;
        self.target = 0.0;
        self.lag.prepare(host_hz);
        self.lag.reset();
    }

    /// Program IV's base clock from TIME's knob position (0..1).
    pub(super) fn stretch(time01: f32) -> f32 {
        ((0.5 - time01) * 2.0 * IV_OCTAVES).exp2()
    }

    /// One frame's clock. `shape`: 0 sine, 1 square, 2 random.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tick(
        &mut self,
        base: f32,
        shift: f32,
        ramp_ms: f32,
        held: bool,
        depth: f32,
        rate_hz: f32,
        shape: usize,
    ) -> f32 {
        let step = 1000.0 / (ramp_ms * self.host_hz);
        self.dive = if held { (self.dive + step).min(1.0) } else { (self.dive - step).max(0.0) };
        let octaves = base.log2() + (shift.log2() - base.log2()) * ease(self.dive);

        self.phase += rate_hz / self.host_hz;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= 1.0;
        }
        let raw = match shape {
            0 => (2.0 * core::f32::consts::PI * self.phase).sin(),
            1 => {
                if self.phase < 0.5 { 1.0 } else { -1.0 }
            }
            _ => {
                if wrapped {
                    self.held = self.target;
                    self.target = self.noise();
                }
                // A smoothed sample-and-hold: glide to each new value
                // over the first quarter of its cycle.
                let q = (self.phase * 4.0).min(1.0);
                self.held + (self.target - self.held) * ease(q)
            }
        };
        let lfo = self.lag.tick(raw);
        (octaves + depth * depth * MOD_OCTAVES * lfo).exp2()
    }

    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Smoothstep — the dive leaves and lands gently.
#[inline]
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}
