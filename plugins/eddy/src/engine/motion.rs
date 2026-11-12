//! Motion — the read head's breath: one LFO shaped by a recipe.
//! The vintage names are RECIPES over the kernel basis (shape ∘ lag
//! ∘ drift), never new oscillator kinds: JUNO is a triangle through
//! a symmetric ~60 ms lag (RC-rounded corners); VIBE is a sine
//! through a 40/160 ms lag — the lamp, bright fast, dim slow. The
//! constants are CONTROL-rate: sized against ~1 s cycles, not audio
//! samples (the visibility law keeps them honest).

use stave_dsp::{Lag, Lfo, LfoShape};

use crate::params::EddyParams;

/// Full-depth wobble span, ± around the dialed time, in ms — deep
/// enough for tape-warble territory at the top of the knob.
pub(super) const DEPTH_MS: f32 = 18.0;

struct Recipe {
    shape: LfoShape,
    lag: Option<(f32, f32)>, // (rise_ms, fall_ms)
    drift: f32,
}

/// FLAVOR is the amount of analog character, from zero (pristine —
/// JUNO at 0 IS the raw triangle, a published law) upward. Two
/// physics lessons are encoded here, both learned from the user's
/// eyes and ears:
/// * Character is the ASYMMETRY, not the magnitude — a long
///   symmetric lag is just a lowpass that regularizes the wave
///   toward a small sine. So VIBE's rise stays FAST and only the
///   fall grows; JUNO's rounding is capped below mush.
/// * Lag times are FRACTIONS OF THE CYCLE, so the bend looks the
///   same at every rate (a fixed-ms lag vanishes at slow rates and
///   swallows fast ones). The shaped wave is renormalized to full
///   scale downstream — flavor buys skew, never amplitude.
fn recipe(motion: usize, flavor: f32, cycle_ms: f32) -> Recipe {
    let f = flavor.clamp(0.0, 1.0);
    let frac = |x: f32| x * cycle_ms;
    match motion {
        1 => Recipe {
            shape: LfoShape::Triangle,
            lag: None,
            drift: 0.0,
        },
        // JUNO: symmetric RC rounding up to τ = 12% of the cycle —
        // heavily rounded corners, never a collapsed sine.
        2 => Recipe {
            shape: LfoShape::Triangle,
            lag: if f <= 0.001 {
                None
            } else {
                Some((frac(0.12 * f), frac(0.12 * f)))
            },
            drift: 0.6 * f,
        },
        // VIBE: the lamp — brightens in ~2% of the cycle always;
        // dims over up to 40% of it. The skew IS the knob.
        3 => Recipe {
            shape: LfoShape::Sine,
            lag: if f <= 0.001 {
                None
            } else {
                Some((frac(0.02 + 0.02 * f), frac(0.40 * f)))
            },
            drift: 0.8 * f,
        },
        _ => Recipe {
            shape: LfoShape::Sine,
            lag: None,
            drift: 0.0,
        },
    }
}

/// Settle-and-capture one shaped cycle (shared by the display and
/// the makeup computation): the lag runs over the cycle for several
/// passes so slow lamps at fast rates capture their honest shape.
fn simulate_cycle(r_shape: LfoShape, r_lag: Option<(f32, f32)>, cycle_ms: f32, ys: &mut [f32]) {
    let n = ys.len();
    let step_ms = cycle_ms / n as f32;
    let mut sim = Lag::new(0.0, 0.0);
    if let Some((rise, fall)) = r_lag {
        sim.set_times(rise, fall, 1000.0 / step_ms);
    }
    const PASSES: usize = 4;
    for pass in 0..PASSES {
        for (i, slot) in ys.iter_mut().enumerate() {
            let phase = core::f32::consts::TAU * i as f32 / (n - 1) as f32;
            let raw = Lfo::eval(r_shape, phase);
            let shaped = if r_lag.is_some() { sim.tick(raw) } else { raw };
            if pass == PASSES - 1 {
                *slot = shaped;
            }
        }
    }
}

pub(super) struct Motion {
    lfo: Lfo,
    lag: Lag,
    lag_on: bool,
    /// Restores full scale after the lag (flavor buys skew, never
    /// amplitude) — recomputed at block rate from the settled cycle.
    makeup: f32,
    last_motion: usize,
    last_rate: f32,
    last_flavor: f32,
}

impl Motion {
    pub(super) fn new() -> Self {
        Self {
            lfo: Lfo::new(),
            lag: Lag::new(0.0, 0.0),
            lag_on: false,
            makeup: 1.0,
            last_motion: usize::MAX,
            last_rate: -1.0,
            last_flavor: -1.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32, rate_hz: f32) {
        self.lfo.prepare(sr, rate_hz, 0.0);
        self.lag.prepare(sr);
        self.last_motion = usize::MAX; // force recipe re-apply
        self.last_rate = -1.0;
        self.last_flavor = -1.0;
    }

    /// Block-rate pass: re-resolve the recipe only when the selector
    /// or rate moved. Returns true when the picture needs
    /// republishing (the caller owns telemetry).
    pub(super) fn apply_changes(&mut self, p: &EddyParams, sr: f32) -> bool {
        let motion = p.motion.index();
        let rate = p.rate.value();
        let flavor = p.flavor.value();
        if motion == self.last_motion
            && (rate - self.last_rate).abs() <= 0.01
            && (flavor - self.last_flavor).abs() <= 0.005
        {
            return false;
        }
        // Only a RECIPE change resets the lag state — retuning it
        // (flavor/rate drags) is continuous, and a mid-stream reset
        // is an audible wobble jump (found by ear).
        let motion_changed = motion != self.last_motion;
        self.last_motion = motion;
        self.last_rate = rate;
        self.last_flavor = flavor;
        let cycle_ms = 1000.0 / rate.max(0.05);
        let r = recipe(motion, flavor, cycle_ms);
        self.lfo.set_shape(r.shape);
        self.lfo.set_drift(r.drift);
        self.lag_on = r.lag.is_some();
        if let Some((rise, fall)) = r.lag {
            self.lag.set_times(rise, fall, sr);
        }
        if motion_changed {
            self.lag.reset();
        }
        // Makeup from the settled cycle: full-scale skew.
        let mut probe = [0.0f32; 64];
        simulate_cycle(r.shape, r.lag, cycle_ms, &mut probe);
        let peak = probe.iter().fold(0.0f32, |m, y| m.max(y.abs())).max(1e-3);
        self.makeup = (1.0 / peak).min(4.0);
        true
    }

    /// Per-frame: advance and return the shaped wobble in SAMPLES
    /// (± depth·DEPTH_MS worth of read-head travel).
    #[inline]
    pub(super) fn tick(&mut self, rate_hz: f32, depth: f32, sr: f32) -> f32 {
        self.lfo.set_rate(rate_hz);
        let raw = self.lfo.tick();
        let shaped = if self.lag_on {
            self.lag.tick(raw) * self.makeup
        } else {
            raw
        };
        shaped * depth * DEPTH_MS * 0.001 * sr
    }

    /// Current phase, 0..1 through the cycle (the display cursor).
    pub(super) fn phase01(&self) -> f32 {
        (self.lfo.phase() / core::f32::consts::TAU).rem_euclid(1.0)
    }

    /// Draw one SHAPED cycle at the current recipe and rate — what
    /// the read head actually sees. The lag is simulated over the
    /// cycle and settled for several passes (slow lamps at fast
    /// rates legitimately compress).
    pub(super) fn render_cycle(&self, ys: &mut [f32], _sr: f32) {
        let cycle_ms = 1000.0 / self.last_rate.max(0.05);
        let r = recipe(self.last_motion.min(3), self.last_flavor.max(0.0), cycle_ms);
        simulate_cycle(r.shape, r.lag, cycle_ms, ys);
        // The display shows what the read head sees: full scale.
        for y in ys.iter_mut() {
            *y = (*y * self.makeup).clamp(-1.0, 1.0);
        }
    }
}
