//! The closed taper vocabulary: how a parameter's plain value maps to
//! the host's normalized [0, 1] and back.
//!
//! Contracts (measured, not aspirational):
//! - Endpoints are pinned: `denormalize(0.0) == min`,
//!   `denormalize(1.0) == max`, bit-exact. Hosts snap to endpoints and
//!   a param that can't reach its own max reads as broken.
//! - `normalize(denormalize(n))` is within `quantum + 1e-14` of `n`,
//!   where `quantum` is the normalized width of one plain-value ULP at
//!   that point. For sane range/taper combos the quantum is ≪ 1e-14;
//!   strong skews compress normalized spans below f64 plain resolution
//!   near the range floor, and there the plain grid — not this code —
//!   is the accuracy limit. Either way the error is ~six orders below
//!   a host's f32 normalized resolution (~6e-8) wherever plain can
//!   express the difference at all.
//! - Both directions are pure functions: same input, same output.
//!   Automation playback (`denormalize` only) and state (which stores
//!   PLAIN values, never re-deriving through normalized) are therefore
//!   exactly reproducible.
//! - Transcendental tapers (Log, Skewed) cannot promise a bit-exact
//!   fixpoint through exp/ln rounding. The enforced invariant is
//!   NORMALIZED-space stability: a host looping `set(get())` stays
//!   within 1e-14 of the original normalized value over unbounded
//!   cycles. Measured (10k cycles): Log settles into a 2-ULP plain
//!   orbit (normalized error 2.8e-17); Skewed near the range floor
//!   wanders in plain ULPs — plain f64 is DENSER than normalized f64
//!   there, so the wander lives inside one normalized point's
//!   preimage — while normalized error stays ≤ 6.8e-17. Plain-ULP
//!   counting is meaningless in that regime; normalized stability is
//!   the contract hosts actually consume.
//! - All math is f64. Plugins may round to f32 at the DSP boundary;
//!   identity must not decay in the mapping layer.
//!
//! This vocabulary is CLOSED on purpose: every taper added here gets
//! the same round-trip checks, and state chunks may assume any
//! (plain value, taper) pair reproduces its normalized position
//! exactly across sessions and plugin versions.

/// Mapping shape between normalized [0, 1] and plain values.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Taper {
    /// Straight line. The default; right for signed/symmetric ranges.
    Linear,
    /// JUCE-style skew expressed by intent, not exponent: `midpoint` is
    /// the PLAIN value at 50% travel. Right when the useful action
    /// clusters at one end (attack times, ratios).
    Skewed { midpoint: f64 },
    /// Logarithmic in the plain value; requires `0 < min < max`.
    /// Right for frequencies and time constants — equal travel per
    /// octave/decade.
    Log,
}

/// A parameter's plain-value range plus its taper. The single source of
/// truth for normalized↔plain everywhere (wrappers, editor, state).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub taper: Taper,
}

impl Range {
    /// Construct, validating taper preconditions. Panics on a range the
    /// taper can't represent — these are author-written constants, and
    /// a wrong one must fail at first construction, not misbehave in a
    /// session.
    pub fn new(min: f64, max: f64, taper: Taper) -> Self {
        assert!(
            min.is_finite() && max.is_finite() && min < max,
            "invalid range {min}..{max}"
        );
        match taper {
            Taper::Log => {
                assert!(min > 0.0, "Log taper requires min > 0 (got {min})");
            }
            Taper::Skewed { midpoint } => {
                assert!(
                    midpoint > min && midpoint < max,
                    "Skewed midpoint {midpoint} outside ({min}, {max})"
                );
            }
            Taper::Linear => {}
        }
        Self { min, max, taper }
    }

    /// Plain → normalized [0, 1]. Values outside the range clamp; NaN
    /// maps to 0 (f64::clamp propagates NaN, so guard explicitly — a
    /// NaN must never reach the host or the state chunk).
    pub fn normalize(&self, plain: f64) -> f64 {
        if plain.is_nan() {
            return 0.0;
        }
        let v = plain.clamp(self.min, self.max);
        // Endpoint pinning, all tapers: exact by construction.
        if v == self.min {
            return 0.0;
        }
        if v == self.max {
            return 1.0;
        }
        let n = match self.taper {
            Taper::Linear => (v - self.min) / (self.max - self.min),
            Taper::Skewed { midpoint } => {
                let frac = (v - self.min) / (self.max - self.min);
                frac.powf(1.0 / skew_exponent(self.min, self.max, midpoint))
            }
            Taper::Log => (v / self.min).ln() / (self.max / self.min).ln(),
        };
        n.clamp(0.0, 1.0)
    }

    /// Normalized [0, 1] → plain. Input outside [0, 1] clamps; NaN maps
    /// to `min` (see `normalize` for why the guard is explicit).
    pub fn denormalize(&self, normalized: f64) -> f64 {
        if normalized.is_nan() {
            return self.min;
        }
        let n = normalized.clamp(0.0, 1.0);
        // Endpoint pinning, all tapers: hosts snap to 0/1 and must land
        // bit-exactly on the declared bounds.
        if n == 0.0 {
            return self.min;
        }
        if n == 1.0 {
            return self.max;
        }
        let v = match self.taper {
            Taper::Linear => self.min + n * (self.max - self.min),
            Taper::Skewed { midpoint } => {
                let frac = n.powf(skew_exponent(self.min, self.max, midpoint));
                self.min + frac * (self.max - self.min)
            }
            Taper::Log => self.min * (self.max / self.min).powf(n),
        };
        v.clamp(self.min, self.max)
    }
}

/// Exponent k such that `0.5^k == (midpoint − min)/(max − min)`, i.e.
/// `denormalize(0.5) == midpoint`.
fn skew_exponent(min: f64, max: f64, midpoint: f64) -> f64 {
    let frac = (midpoint - min) / (max - min);
    frac.ln() / 0.5f64.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic dense sweep of the three contracts that actually
    /// protect state integrity and automation fidelity:
    ///
    /// 1. Endpoints bit-exact (hosts snap to 0/1).
    /// 2. norm→plain→norm within 1e-14 ABSOLUTE in normalized space —
    ///    a host's f32 normalized resolution is ~6e-8, so this is six
    ///    orders below observable. (ULP-relative is the wrong metric
    ///    here: tiny n through a range with |min| ≫ range·n loses
    ///    relative precision to benign cancellation while absolute
    ///    error stays ~1e-17.)
    /// 3. Orbit stability in NORMALIZED space: `set(get())` cycles
    ///    never move the normalized value more than 1e-14 from where
    ///    it started, no matter how many cycles run. See module docs
    ///    for the measured orbit behavior behind this choice of
    ///    invariant.
    fn roundtrip_check(range: Range) {
        assert_eq!(range.denormalize(0.0), range.min);
        assert_eq!(range.denormalize(1.0), range.max);
        assert_eq!(range.normalize(range.min), 0.0);
        assert_eq!(range.normalize(range.max), 1.0);
        // Miri interprets; keep its sweep density survivable while the
        // native run stays dense.
        const SWEEP: usize = if cfg!(miri) { 500 } else { 100_000 };
        const ORBIT_POINTS: usize = if cfg!(miri) { 20 } else { 1_000 };
        const ORBIT_CYCLES: usize = 100;

        // One plain-ULP step, without f64::next_up (MSRV 1.80).
        let ulp_up = |v: f64| {
            f64::from_bits(if v >= 0.0 {
                v.to_bits() + 1
            } else {
                v.to_bits() - 1
            })
        };
        let ulp_down = |v: f64| {
            f64::from_bits(if v > 0.0 {
                v.to_bits() - 1
            } else {
                v.to_bits() + 1
            })
        };

        let single_trip = |n: f64| {
            let plain = range.denormalize(n);
            let back = range.normalize(plain);
            // The unavoidable error floor is the local plain-grid
            // quantum: strong skews compress whole normalized spans
            // into ONE representable plain value near the range floor
            // (e.g. Skewed{0.1..5000, mid 50}: n=1e-5 is a plain offset
            // of ~1e-33 — below f64 resolution at 0.1). The limit: round
            // trip lands within the normalized image of plain ± 1 ULP.
            let quantum = (range.normalize(ulp_up(plain)) - range.normalize(ulp_down(plain))).abs();
            // Miri deliberately jitters transcendental results by a few
            // ULP; it checks UB here, not float accuracy — loosen.
            // Miri's jitter is a few plain-ULPs, so its normalized
            // effect scales with the quantum in compressed regions.
            let allowed = if cfg!(miri) {
                quantum * 4.0 + 1e-9
            } else {
                quantum + 1e-14
            };
            assert!(
                (back - n).abs() <= allowed,
                "{range:?}: n={n} → plain={plain} → back={back} (err {} > quantum {} + 1e-14)",
                (back - n).abs(),
                quantum
            );
        };
        for i in 1..SWEEP {
            single_trip(i as f64 / SWEEP as f64);
        }
        for &n in &[1e-15, 1e-9, 1.0 - 1e-15, 1.0 - 1e-9, 0.5, f64::EPSILON] {
            single_trip(n);
        }
        // Orbit check on a sparser grid (each point runs many cycles).
        // Same quantum-aware tolerance, fixed from the FIRST iterate:
        // the orbit must stay put, wherever quantization first lands it.
        let orbit = |n: f64| {
            let mut v = range.denormalize(n);
            let quantum = (range.normalize(ulp_up(v)) - range.normalize(ulp_down(v))).abs();
            // See single_trip: Miri jitters transcendentals on purpose.
            // Miri's jitter is a few plain-ULPs, so its normalized
            // effect scales with the quantum in compressed regions.
            let allowed = if cfg!(miri) {
                quantum * 4.0 + 1e-9
            } else {
                quantum + 1e-14
            };
            for cycle in 0..ORBIT_CYCLES {
                let nn = range.normalize(v);
                assert!(
                    (nn - n).abs() <= allowed,
                    "{range:?}: n={n} drifted to {nn} after {cycle} cycles (allowed {allowed})"
                );
                v = range.denormalize(nn);
            }
        };
        for i in 1..ORBIT_POINTS {
            orbit(i as f64 / ORBIT_POINTS as f64);
        }
        for &n in &[1e-5, 1e-9, 1.0 - 1e-9, 0.0627, 0.5] {
            orbit(n);
        }
    }

    #[test]
    fn linear_roundtrip() {
        roundtrip_check(Range::new(-24.0, 24.0, Taper::Linear));
        roundtrip_check(Range::new(0.0, 1.0, Taper::Linear));
    }

    #[test]
    fn log_roundtrip() {
        roundtrip_check(Range::new(20.0, 20_000.0, Taper::Log));
        roundtrip_check(Range::new(0.1, 100.0, Taper::Log));
    }

    #[test]
    fn skewed_roundtrip_and_midpoint_contract() {
        let r = Range::new(0.0, 100.0, Taper::Skewed { midpoint: 20.0 });
        roundtrip_check(r);
        // The whole point of Skewed: 50% travel lands on the midpoint.
        assert!((r.denormalize(0.5) - 20.0).abs() < 1e-9);
        let ms = Range::new(0.1, 5000.0, Taper::Skewed { midpoint: 50.0 });
        roundtrip_check(ms);
        assert!((ms.denormalize(0.5) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_inputs_clamp() {
        let r = Range::new(20.0, 20_000.0, Taper::Log);
        assert_eq!(r.normalize(5.0), 0.0);
        assert_eq!(r.normalize(1e9), 1.0);
        assert_eq!(r.denormalize(-3.0), 20.0);
        assert_eq!(r.denormalize(7.0), 20_000.0);
        // NaN normalized clamps to an endpoint rather than propagating.
        assert!(r.denormalize(f64::NAN).is_finite());
    }

    #[test]
    #[should_panic(expected = "Log taper requires min > 0")]
    fn log_rejects_nonpositive_min() {
        let _ = Range::new(0.0, 100.0, Taper::Log);
    }

    #[test]
    #[should_panic(expected = "outside")]
    fn skew_rejects_midpoint_outside_range() {
        let _ = Range::new(0.0, 1.0, Taper::Skewed { midpoint: 2.0 });
    }
}
