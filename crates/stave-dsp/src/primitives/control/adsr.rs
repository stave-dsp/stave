//! `Adsr` — the envelope GENERATOR (EnvelopeFollower is the detector,
//! the opposite direction): gate-driven Idle → Attack → Decay →
//! Release over rate-derived one-pole segments.
//!
//! Segment semantics, stated because the two conventions differ:
//! * ATTACK `ms` is the ARRIVAL time 0 → 1. The segment aims above
//!   the ceiling (×1.3) and switches at 1.0 — the analog snap: a
//!   one-pole aimed exactly at 1 never arrives, and a linear ramp
//!   sounds like a fade, not a pluck. τ = ms / ln(1.3/0.3) makes the
//!   stated ms the measured arrival.
//! * DECAY and RELEASE `ms` are TIME CONSTANTS (τ), matching the
//!   house convention everywhere else (`Smooth::Exp`, `Lag`): after
//!   one τ the segment has closed 63% of its distance.
//!
//! There is no separate Sustain stage: Decay aims at the sustain
//! level forever, which IS sustain once it arrives. Retrigger starts
//! Attack from the CURRENT level (no click); `choke` is immediate
//! silence (the rack's kill semantics — a click is the caller's
//! accepted trade).

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::tau_coeff;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Segment {
    Idle,
    Attack,
    Decay,
    Release,
}

/// Attack overshoot target and the τ correction that makes the
/// stated attack ms the actual 0→1 arrival: 1.3·(1−e^(−t/τ)) = 1.0
/// at t = τ·ln(1.3/0.3).
const ATTACK_AIM: f32 = 1.3;
const ATTACK_LN: f32 = 1.466_337; // ln(1.3 / 0.3)

/// Below this the envelope is silence (−80 dB) and Release parks.
const IDLE_EPS: f32 = 1e-4;

/// The four envelope settings. See the module header for what each
/// `ms` means.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AdsrConfig {
    pub attack_ms: f32,
    pub decay_ms: f32,
    /// Level Decay settles at, 0..=1.
    pub sustain: f32,
    pub release_ms: f32,
}

#[derive(Copy, Clone, Debug)]
pub struct Adsr {
    segment: Segment,
    config: AdsrConfig,
    sample_rate: f32,
    level: f32,
    a_coeff: f32,
    d_coeff: f32,
    r_coeff: f32,
    sustain: f32,
}

impl Adsr {
    pub fn new() -> Self {
        Self {
            segment: Segment::Idle,
            config: AdsrConfig {
                attack_ms: 0.0,
                decay_ms: 0.0,
                sustain: 1.0,
                release_ms: 0.0,
            },
            sample_rate: 48_000.0,
            level: 0.0,
            a_coeff: 1.0,
            d_coeff: 1.0,
            r_coeff: 1.0,
            sustain: 1.0,
        }
    }

    /// New settings (block rate is fine — the segment state itself is
    /// untouched, so times move mid-note without clicks).
    pub fn set(&mut self, config: &AdsrConfig) {
        debug_assert!(
            config.attack_ms >= 0.0 && config.decay_ms >= 0.0 && config.release_ms >= 0.0,
            "{config:?}"
        );
        self.config = *config;
        let sample_rate = self.sample_rate;
        self.a_coeff = tau_coeff(config.attack_ms / ATTACK_LN * 0.001, sample_rate);
        self.d_coeff = tau_coeff(config.decay_ms * 0.001, sample_rate);
        self.r_coeff = tau_coeff(config.release_ms * 0.001, sample_rate);
        debug_assert!(
            (0.0..=1.0).contains(&config.sustain),
            "sustain {}",
            config.sustain
        );
        self.sustain = config.sustain;
    }

    /// Gate on: Attack from the CURRENT level (retrigger-safe).
    pub fn gate_on(&mut self) {
        self.segment = Segment::Attack;
    }

    /// Gate off: Release from wherever the envelope is.
    pub fn gate_off(&mut self) {
        if self.segment != Segment::Idle {
            self.segment = Segment::Release;
        }
    }

    /// Immediate silence (choke semantics).
    pub fn choke(&mut self) {
        self.segment = Segment::Idle;
        self.level = 0.0;
    }

    /// Still sounding? False once Release (or a zero-sustain Decay)
    /// parks below the idle floor.
    #[inline]
    pub fn active(&self) -> bool {
        self.segment != Segment::Idle
    }

    /// The current envelope level — the steal policy's loudness.
    #[inline]
    pub fn level(&self) -> f32 {
        self.level
    }
}

impl Stage for Adsr {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let config = self.config;
        self.set(&config);
        self.choke();
    }
    fn reset(&mut self) {
        self.choke();
    }
    /// One envelope sample; the input is ignored (the gate drives it).
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        match self.segment {
            Segment::Idle => {}
            Segment::Attack => {
                self.level += self.a_coeff * (ATTACK_AIM - self.level);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.segment = Segment::Decay;
                }
            }
            Segment::Decay => {
                self.level += self.d_coeff * (self.sustain - self.level);
                // A zero-sustain decay is a release in disguise:
                // park it, or the voice reads active forever.
                if self.sustain < IDLE_EPS && self.level < IDLE_EPS {
                    self.choke();
                }
            }
            Segment::Release => {
                self.level += self.r_coeff * (0.0 - self.level);
                if self.level < IDLE_EPS {
                    self.choke();
                }
            }
        }
        self.level
    }
    /// A generator: it sounds for as long as its gate holds.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn run(e: &mut Adsr, n: usize) -> f32 {
        let mut last = 0.0;
        for _ in 0..n {
            last = e.tick(0.0);
        }
        last
    }

    #[test]
    fn attack_ms_is_the_arrival_time() {
        let mut e = Adsr::new();
        e.prepare(SR);
        e.set(&AdsrConfig {
            attack_ms: 10.0,
            decay_ms: 100.0,
            sustain: 0.5,
            release_ms: 50.0,
        });
        e.gate_on();
        // 1.0 must be reached at the stated 10 ms (±5% — one-pole
        // discretization), not the ~3× later a τ-aimed pole gives.
        let n_expected = (0.010 * SR) as usize;
        let mut arrived_at = None;
        for i in 0..(2 * n_expected) {
            if e.tick(0.0) >= 1.0 {
                arrived_at = Some(i);
                break;
            }
        }
        let at = arrived_at.expect("attack never arrived");
        let err = (at as f32 - n_expected as f32).abs() / n_expected as f32;
        assert!(
            err < 0.05,
            "arrival {at} vs expected {n_expected} ({err:.3})"
        );
    }

    #[test]
    fn decay_settles_at_sustain_and_release_tau_is_honest() {
        let mut e = Adsr::new();
        e.prepare(SR);
        e.set(&AdsrConfig {
            attack_ms: 1.0,
            decay_ms: 20.0,
            sustain: 0.6,
            release_ms: 30.0,
        });
        e.gate_on();
        let settled = run(&mut e, (0.5 * SR) as usize);
        assert!((settled - 0.6).abs() < 1e-3, "sustain: {settled}");
        e.gate_off();
        // One release τ closes 63% of the distance to zero.
        let after_tau = run(&mut e, (0.030 * SR) as usize);
        let expected = 0.6 * (-1.0f32).exp();
        assert!(
            (after_tau - expected).abs() < 0.02,
            "after τ: {after_tau} vs {expected}"
        );
        run(&mut e, (2.0 * SR) as usize);
        assert!(!e.active(), "release must park at idle");
    }

    #[test]
    fn retrigger_continues_from_current_level() {
        let mut e = Adsr::new();
        e.prepare(SR);
        e.set(&AdsrConfig {
            attack_ms: 5.0,
            decay_ms: 50.0,
            sustain: 0.8,
            release_ms: 100.0,
        });
        e.gate_on();
        run(&mut e, (0.2 * SR) as usize); // at sustain 0.8
        e.gate_off();
        run(&mut e, (0.020 * SR) as usize); // partway down
        let before = e.level();
        assert!(before > 0.1, "test premise: mid-release");
        e.gate_on();
        let first = e.tick(0.0);
        assert!(
            first >= before && (first - before) < 0.01,
            "retrigger jumps: {before} → {first}"
        );
    }

    #[test]
    fn zero_sustain_decay_parks_idle() {
        let mut e = Adsr::new();
        e.prepare(SR);
        e.set(&AdsrConfig {
            attack_ms: 1.0,
            decay_ms: 10.0,
            sustain: 0.0,
            release_ms: 100.0,
        });
        e.gate_on();
        run(&mut e, SR as usize); // 1 s ≫ decay
        assert!(!e.active(), "zero-sustain note must end without a gate_off");
    }

    #[test]
    fn choke_is_immediate() {
        let mut e = Adsr::new();
        e.prepare(SR);
        e.set(&AdsrConfig {
            attack_ms: 1.0,
            decay_ms: 50.0,
            sustain: 0.9,
            release_ms: 500.0,
        });
        e.gate_on();
        run(&mut e, 4_800);
        e.choke();
        assert_eq!(e.tick(0.0), 0.0);
        assert!(!e.active());
    }
}
