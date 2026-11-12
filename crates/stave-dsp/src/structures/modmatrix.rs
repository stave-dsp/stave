//! The modulation matrix: a kernel [`Router`] whose sources are
//! [`Driver`]s and whose targets are a [`Chain`]'s stage axes.
//!
//! Runs at base rate outside any oversampler; writes pending stage
//! frames only (the `CurveStage` commit discipline keeps ADAA honest).
//! Targets cover the [`Frame`] axes plus tilt — the spectral axis
//! (level-dependent frequency response: Miller darkening, saturating-
//! inductor EQ).

use crate::behavioral::curve::Frame;
use crate::kernel::control::{Full, Route, Router, Source};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::control::envelope::{EnvelopeFollower, EnvelopeTimes};
use crate::primitives::control::lfo::Lfo;
use crate::primitives::control::sub::SubOctave;
use crate::primitives::filters::biquad::FilterSpec;
use crate::structures::chain::StageKind;
use crate::structures::chain::{Chain, StageId};

/// A modulation source — the generalization that makes every sideband
/// phenomenon a route recipe instead of plugin code.
///
/// * `Env` — the classic detector-following source (state ≥ 0).
/// * `Lfo` — a periodic source (state ∈ [−1, 1] · scale). With
///   `env_scale`, the LFO is multiplied by an envelope of the detector:
///   supply ripple is proportional to current draw, so quiet passages
///   stay clean — the difference between "vintage ghost" and "vibrato".
pub enum Driver {
    Env(EnvelopeFollower),
    Lfo {
        lfo: Lfo,
        env_scale: bool,
        env: EnvelopeFollower,
        last: f32,
    },
    /// Period-tracking divider: a bipolar source at HALF the program's
    /// fundamental. A Gain route driven by it puts energy at 1.5·f0,
    /// 2.5·f0, … — between the integer harmonics. Feed it the SIGNED
    /// signal as its detector, not a rectified level.
    Sub(SubOctave),
}

impl From<EnvelopeFollower> for Driver {
    fn from(c: EnvelopeFollower) -> Self {
        Driver::Env(c)
    }
}

impl Driver {
    /// Free-running periodic source (bipolar output).
    pub fn lfo(hz: f32) -> Self {
        Driver::Lfo {
            lfo: Lfo::new(hz),
            env_scale: false,
            env: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 5.0,
                release_ms: 60.0,
                slow_release_ms: None,
            }),
            last: 0.0,
        }
    }
    /// Periodic source scaled by the detector's envelope (attack/release
    /// in ms) — ripple that rides program heat.
    pub fn lfo_env_scaled(hz: f32, attack_ms: f32, release_ms: f32) -> Self {
        Driver::Lfo {
            lfo: Lfo::new(hz),
            env_scale: true,
            env: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms,
                release_ms,
                slow_release_ms: None,
            }),
            last: 0.0,
        }
    }
    /// Divider driver (see `SubOctave`).
    pub fn sub() -> Self {
        Driver::Sub(SubOctave::new())
    }
    /// Retune the envelope half (no-op timing on a free-running LFO's
    /// phase or a divider; retunes the env-scaling follower where present).
    pub fn set_times(&mut self, times: EnvelopeTimes) {
        match self {
            Driver::Env(c) => c.set_times(times),
            Driver::Lfo { env, .. } => env.set_times(times),
            Driver::Sub(_) => {}
        }
    }
}

impl Stage for Driver {
    fn prepare(&mut self, sample_rate: f32) {
        match self {
            Driver::Env(c) => c.prepare(sample_rate),
            Driver::Lfo { lfo, env, last, .. } => {
                lfo.prepare(sample_rate);
                env.prepare(sample_rate);
                *last = 0.0;
            }
            Driver::Sub(s) => s.prepare(sample_rate),
        }
    }
    fn reset(&mut self) {
        match self {
            Driver::Env(c) => c.reset(),
            Driver::Lfo { env, last, .. } => {
                env.reset();
                *last = 0.0;
            }
            Driver::Sub(s) => s.reset(),
        }
    }
    #[inline]
    fn tick(&mut self, detector: f32) -> f32 {
        match self {
            // An envelope follows the detector's level.
            Driver::Env(c) => c.tick(detector.abs()),
            Driver::Lfo {
                lfo,
                env_scale,
                env,
                last,
            } => {
                let osc = lfo.tick(0.0);
                let s = if *env_scale {
                    osc * env.tick(detector.abs())
                } else {
                    osc
                };
                *last = s;
                s
            }
            Driver::Sub(s) => s.tick(detector),
        }
    }
    fn tail(&self) -> Tail {
        match self {
            Driver::Env(c) => c.tail(),
            Driver::Lfo { .. } => Tail::Infinite,
            Driver::Sub(s) => s.tail(),
        }
    }
}

impl Source for Driver {
    fn state(&self) -> f32 {
        match self {
            Driver::Env(c) => c.state(),
            Driver::Lfo { last, .. } => *last,
            Driver::Sub(s) => s.state(),
        }
    }
}

/// What a route writes on its target stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Frame.bias += amount·state
    Bias,
    /// Frame.drive ×= (1 + amount·state)
    Drive,
    /// Frame.gain ×= (1 − amount·state), floored at 0
    Gain,
    /// Filter shelf/peak gain += amount·state dB (the spectral axis)
    TiltDb,
}

/// Where a [`ModMatrix`] route lands: one axis of one chain stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageTarget {
    pub stage: StageId,
    pub target: Target,
}

/// Fixed-capacity modulation router. Each `tick`, drivers advance from
/// their detectors, then routes are applied on top of caller-provided
/// base values (`base_frames` = what the knobs say; routes displace from
/// there, so knob writes and modulation never fight).
///
/// The matrix is a kernel [`Router`] — sources and edges as data, the
/// one [`Route`] type — plus the chain-typed apply pass.
pub struct ModMatrix<const D: usize, const R: usize> {
    router: Router<Driver, StageTarget, D, R>,
}

impl<const D: usize, const R: usize> ModMatrix<D, R> {
    /// Accepts anything convertible to a Driver (an
    /// `[EnvelopeFollower; D]` among them).
    pub fn new<T: Into<Driver>>(drivers: [T; D]) -> Self {
        Self {
            router: Router::new(drivers.map(Into::into)),
        }
    }
    pub fn drivers(&mut self) -> &mut [Driver; D] {
        &mut self.router.sources
    }
    /// The underlying router (its live routes, for reflection).
    pub fn router(&self) -> &Router<Driver, StageTarget, D, R> {
        &self.router
    }
    /// Add a route; [`Full`] when all `R` slots are taken.
    pub fn push_route(&mut self, route: Route<StageTarget>) -> Result<(), Full> {
        self.router.push_route(route)
    }
    pub fn clear_routes(&mut self) {
        self.router.clear_routes();
    }
    pub fn prepare(&mut self, sample_rate: f32) {
        self.router.prepare(sample_rate);
    }
    pub fn reset(&mut self) {
        self.router.reset();
    }

    /// Advance drivers only (one detector each). Use with `apply` when the
    /// same driver states feed MULTIPLE chains (e.g. mid/side) — ticking
    /// per chain would double the effective rate and break the τ labels.
    pub fn tick_states(&mut self, detectors: &[f32; D]) -> [f32; D] {
        self.router.tick_states(detectors)
    }

    /// Apply routes for given driver states: frames start from
    /// `base_frames` (the knob values) and routes displace them.
    pub fn apply<const N: usize>(
        &self,
        states: &[f32; D],
        chain: &mut Chain<N>,
        base_frames: &[(StageId, Frame)],
        base_tilt: &[(StageId, FilterSpec)],
    ) {
        // Start every routed stage from its base, then displace.
        for &(id, f) in base_frames {
            let mut frame = f;
            for r in self.router.routes() {
                if r.target.stage != id {
                    continue;
                }
                let s = states[r.source.min(D - 1)];
                match r.target.target {
                    Target::Bias => frame.bias += r.amount * s,
                    Target::Drive => frame.drive *= (1.0 + r.amount * s).max(1e-3),
                    Target::Gain => frame.gain *= (1.0 - r.amount * s).max(0.0),
                    Target::TiltDb => {}
                }
            }
            if let Some(StageKind::Curve(cs)) = chain.stage_mut(id) {
                cs.set_pending(frame);
            }
        }
        for &(id, spec) in base_tilt {
            let mut db_add = 0.0f32;
            for r in self.router.routes() {
                if r.target.stage == id && r.target.target == Target::TiltDb {
                    db_add += r.amount * states[r.source.min(D - 1)];
                }
            }
            if let Some(StageKind::Filter(fs)) = chain.stage_mut(id) {
                let spec = match spec {
                    FilterSpec::HighShelf { hz, db } => FilterSpec::HighShelf {
                        hz,
                        db: db + db_add,
                    },
                    FilterSpec::LowShelf { hz, db } => FilterSpec::LowShelf {
                        hz,
                        db: db + db_add,
                    },
                    FilterSpec::Peak { hz, db, q } => FilterSpec::Peak {
                        hz,
                        db: db + db_add,
                        q,
                    },
                    other => other,
                };
                fs.set(spec);
            }
        }
    }

    /// Convenience: tick drivers and apply to one chain.
    pub fn tick<const N: usize>(
        &mut self,
        detectors: &[f32; D],
        chain: &mut Chain<N>,
        base_frames: &[(StageId, Frame)],
        base_tilt: &[(StageId, FilterSpec)],
    ) {
        let states = self.tick_states(detectors);
        self.apply(&states, chain, base_frames, base_tilt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavioral::curve::CurveStage;
    use crate::behavioral::shapes;

    #[test]
    fn envelope_times_are_honest_across_rates() {
        for rate in [44_100.0f32, 48_000.0, 96_000.0] {
            let mut c = EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 5.0,
                release_ms: 120.0,
                slow_release_ms: None,
            });
            c.prepare(rate);
            for _ in 0..(rate * 0.5) as usize {
                let _ = c.tick(0.8);
            }
            let v0 = c.state();
            let mut k = 0usize;
            for i in 0..rate as usize {
                let _ = c.tick(0.0);
                if c.state() < v0 * (-1.0f32).exp() {
                    k = i;
                    break;
                }
            }
            let tau = k as f32 / rate * 1000.0;
            assert!(
                (tau - 120.0).abs() < 12.0,
                "release τ {tau} ms at {rate} (label 120)"
            );
        }
    }

    #[test]
    fn routes_displace_from_base_and_recover() {
        // One driver → bias on a curve stage: under sustained detector the
        // pending bias rises above base; after silence it returns.
        let mut ch = Chain::<2>::new();
        let id = ch
            .push(StageKind::Curve(CurveStage::from_fn(
                -4.0,
                4.0,
                0.0,
                shapes::knee(2.0),
            )))
            .unwrap();
        ch.prepare(48_000.0);
        let mut mm = ModMatrix::<1, 4>::new([EnvelopeFollower::new(EnvelopeTimes {
            attack_ms: 5.0,
            release_ms: 60.0,
            slow_release_ms: None,
        })]);
        mm.prepare(48_000.0);
        mm.push_route(Route {
            source: 0,
            target: StageTarget {
                stage: id,
                target: Target::Bias,
            },
            amount: 0.5,
        })
        .unwrap();
        let base = [(
            id,
            Frame {
                bias: 0.1,
                drive: 2.0,
                gain: 1.0,
            },
        )];
        for _ in 0..24_000 {
            mm.tick(&[0.8], &mut ch, &base, &[]);
        }
        let hot = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
            cs.pending_mut().bias
        } else {
            0.0
        };
        assert!(hot > 0.3, "bias should rise under level: {hot}");
        for _ in 0..48_000 {
            mm.tick(&[0.0], &mut ch, &base, &[]);
        }
        let cold = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
            cs.pending_mut().bias
        } else {
            0.0
        };
        assert!(
            (cold - 0.1).abs() < 0.02,
            "bias should return to base: {cold}"
        );
    }
}
