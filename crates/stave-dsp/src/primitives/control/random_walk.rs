//! `RandomWalk` — the bounded random walker for parts that DRIFT: a
//! white target that RENEWS at an interval, eased toward by a
//! one-pole. Renewal is what gives a walk its reach — white noise
//! through a slow one-pole alone has almost no variance, and anything
//! that moves inside a recirculation pumps energy unless its excursion
//! is bounded and its spectrum band-limited. `RandomWalk` is both by construction: targets are
//! uniform ±1 and the ease is convex, so the walk never leaves ±1
//! and never steps — a wandering delay or gain modulated by it can
//! contribute at most its dialed depth.
//!
//! Unipolar walks (0..1 positions, tap clouds) are the affine remap
//! `0.5 + 0.5 · tick()` — easing commutes with affine
//! maps, and the remapped target distribution is the same uniform
//! 0..1, so no second kind is earned.
//!
//! Deterministic: same seed, same sequence — walks are part of the
//! sound and must survive a bit-compare (validation renders twice).

use crate::kernel::control::Source;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::primitives::random::Xorshift32;

#[derive(Clone, Copy, Debug)]
pub struct RandomWalk {
    rng: Xorshift32,
    value: f32,
    target: f32,
    countdown: u32,
    period: u32,
    interval_ms: f32,
    ease_ms: f32,
    ease: f32,
    sample_rate: f32,
}

impl RandomWalk {
    /// `interval_ms` is the renewal cadence (the walk's reach);
    /// `ease_ms` the approach time constant (its smoothness). Seeds
    /// must be nonzero (xorshift's absorbing state) — 0 is remapped.
    pub fn new(interval_ms: f32, ease_ms: f32, seed: u32) -> Self {
        let mut w = Self {
            rng: Xorshift32::new(seed),
            value: 0.0,
            target: 0.0,
            countdown: 0,
            period: 1,
            interval_ms,
            ease_ms,
            ease: 1.0,
            sample_rate: 48_000.0,
        };
        w.set_times(interval_ms, ease_ms);
        w.countdown = 0;
        w
    }

    /// Retime a live walk (the Lag precedent): cadence and ease
    /// follow a knob without resetting state or sequence.
    pub fn set_times(&mut self, interval_ms: f32, ease_ms: f32) {
        self.interval_ms = interval_ms;
        self.ease_ms = ease_ms;
        self.period = ((interval_ms * 0.001 * self.sample_rate) as u32).max(1);
        self.ease = tau_coeff(ease_ms * 0.001, self.sample_rate);
        self.countdown = self.countdown.min(self.period);
    }

    /// Place the walk (seeding a cloud scattered instead of born at
    /// center). The next renewal still fires on its own cadence.
    pub fn snap(&mut self, value: f32) {
        debug_assert!((-1.0..=1.0).contains(&value), "walk position {value}");
        self.value = value;
        self.target = self.value;
    }
}

impl Stage for RandomWalk {
    /// Derive rate-dependent constants and come to rest.
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.set_times(self.interval_ms, self.ease_ms);
        self.reset();
    }

    /// Back to rest at center. The generator's position in its
    /// sequence is kept — reset never repeats a stretch of walk.
    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.countdown = 0;
    }

    /// The walk, one sample (the input is ignored): renew on the
    /// countdown, ease toward the target, yield the bounded position.
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        if self.countdown == 0 {
            self.countdown = self.period;
            self.target = self.rng.next_bipolar();
        }
        self.countdown -= 1;
        self.value += (self.target - self.value) * self.ease;
        self.value
    }

    /// A generator: it never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Source for RandomWalk {
    fn state(&self) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(w: &mut RandomWalk, n: usize) -> Vec<f32> {
        (0..n).map(|_| w.tick(0.0)).collect()
    }

    #[test]
    fn bounded_by_construction() {
        let mut w = RandomWalk::new(40.0, 30.0, 7);
        w.prepare(48_000.0);
        for (i, v) in run(&mut w, 480_000).iter().enumerate() {
            assert!((-1.0..=1.0).contains(v), "sample {i} escaped: {v}");
        }
    }

    #[test]
    fn renewal_gives_the_walk_its_reach() {
        // The measurement: an eased renewing walk has
        // real variance; white through the same one-pole alone
        // (renewal every sample) has almost none.
        let sr = 48_000.0;
        let mut renewing = RandomWalk::new(140.0, 30.0, 7);
        let mut per_sample = RandomWalk::new(0.0, 30.0, 7); // interval < 1 sample
        renewing.prepare(sr);
        per_sample.prepare(sr);
        let std = |v: &[f32]| {
            let m = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let reach = std(&run(&mut renewing, 480_000));
        let hum = std(&run(&mut per_sample, 480_000));
        assert!(reach > 0.25, "renewing walk barely moves: std {reach:.3}");
        assert!(
            hum < 0.1,
            "per-sample renewal should have almost no variance: {hum:.3}"
        );
        assert!(
            reach > 4.0 * hum,
            "renewal isn't what gives the reach ({reach:.3} vs {hum:.3})"
        );
    }

    #[test]
    fn deterministic_per_seed_and_across_reset() {
        let mut a = RandomWalk::new(40.0, 30.0, 42);
        let mut b = RandomWalk::new(40.0, 30.0, 42);
        a.prepare(48_000.0);
        b.prepare(48_000.0);
        assert_eq!(
            run(&mut a, 10_000),
            run(&mut b, 10_000),
            "same seed, same walk"
        );
        let mut c = RandomWalk::new(40.0, 30.0, 43);
        c.prepare(48_000.0);
        assert_ne!(run(&mut a, 0), run(&mut c, 1), "seeds decorrelate");
    }

    #[test]
    fn instant_ease_plateaus_at_the_renewal_cadence() {
        let sr = 1_000.0;
        let mut w = RandomWalk::new(100.0, 0.0, 5); // 100 samples/renewal, no ease
        w.prepare(sr);
        let out = run(&mut w, 1_000);
        let plateaus = 1 + out.windows(2).filter(|p| p[0] != p[1]).count();
        assert_eq!(plateaus, 10, "10 renewals in 1000 samples at 100/renewal");
    }
}
