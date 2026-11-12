//! Shape library — source functions for [`Curve`](crate::behavioral::curve::Curve).
//!
//! Every builder returns `impl Fn(f64) -> f64`, so shapes COMPOSE: any
//! memoryless stack (pre-gain → asymmetric clip → fold → …) collapses into
//! one closure, which tabulates into one Curve with antiderivatives — at
//! which point ADAA and the oversampler make it shippable and the
//! conformance harness decides how far its knobs may travel. Complexity is
//! a build-time cost only: a five-stage composite ticks at the same price
//! as tanh once tabulated.
//!
//! Conventions: shapes are ~unity-slope at 0 (small signals pass clean;
//! the slope normalisation in consumers stays meaningful) and bounded on
//! the Curve domain.

/// Variable-knee symmetric clipper: `x / (1 + |x|^k)^(1/k)`.
/// k = 1 is softer than tanh, k ≈ 2.4 ≈ tanh, k → ∞ is a hard clip at ±1.
/// This is the EDGE control's family.
pub fn knee(k: f64) -> impl Fn(f64) -> f64 {
    let k = k.max(0.5);
    move |x: f64| x / (1.0 + x.abs().powf(k)).powf(1.0 / k)
}

/// Asymmetric clipper: independent ceilings per polarity (diode pairs,
/// grid conduction). `pos_th`/`neg_th` are the clip levels (> 0), `k` the
/// shared knee hardness. Unity slope at 0.
pub fn asym(pos_th: f64, neg_th: f64, k: f64) -> impl Fn(f64) -> f64 {
    debug_assert!(pos_th > 0.0 && neg_th > 0.0, "clip levels must be positive");
    let (p, n) = (pos_th, neg_th);
    let f = knee(k);
    move |x: f64| {
        if x >= 0.0 {
            p * f(x / p)
        } else {
            n * f(x / n)
        }
    }
}

/// Wavefolder: `sin(a·x)/a` with `a = π/2·(1 + 4·depth)` — the /a
/// normalisation makes the small-signal slope EXACTLY unity at every
/// depth (F-list hygiene: the old docstring claimed callers had to
/// normalise; the implementation already does). At depth 0 it is a
/// smooth sine-shaped saturator; as depth rises the waveform folds
/// back on itself — the West-coast spectrum. Deliberately
/// non-monotonic: gate its range with the alias limits, not assumptions.
pub fn fold(depth: f64) -> impl Fn(f64) -> f64 {
    let a = core::f64::consts::FRAC_PI_2 * (1.0 + 4.0 * depth.max(0.0));
    move |x: f64| (a * x).sin() / a // sin(ax)/a: unity slope at 0
}

/// SIGNED Chebyshev recipe through a tanh bound: `Σ wᵢ·Tᵢ₊₁(tanh x)`.
/// `weights[0]` is the fundamental (T₁), `weights[1]` = h2, … Negative
/// weights are allowed — phase-inverted harmonics make notched/metallic
/// recipes unreachable with positive-only weights.
pub fn chebyshev<const N: usize>(weights: [f64; N]) -> impl Fn(f64) -> f64 {
    move |x: f64| {
        let u = x.tanh();
        let mut t0 = 1.0f64;
        let mut t1 = u;
        let mut y = weights[0] * t1;
        for w in weights.iter().skip(1) {
            let t2 = 2.0 * u * t1 - t0;
            y += w * t2;
            t0 = t1;
            t1 = t2;
        }
        // Null the zero-input offset from even terms (silence → silence).
        let mut off = 0.0f64;
        let mut tt0 = 1.0f64;
        let mut tt1 = 0.0f64;
        for w in weights.iter().skip(1) {
            let t2 = -tt0;
            off += w * t2;
            tt0 = tt1;
            tt1 = t2;
        }
        y - off
    }
}

/// Composition: `g ∘ f` (f first). Chain as many as you like — the result
/// is still one memoryless function, i.e. one Curve.
pub fn compose(f: impl Fn(f64) -> f64, g: impl Fn(f64) -> f64) -> impl Fn(f64) -> f64 {
    move |x: f64| g(f(x))
}

/// Input gain ahead of a shape (drive baked into the composite).
pub fn pre_gain(gain: f64, f: impl Fn(f64) -> f64) -> impl Fn(f64) -> f64 {
    move |x: f64| f(gain * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_morphs_soft_to_hard() {
        // Small-signal ~unity for all k; large-signal ceiling → 1; higher
        // k = squarer (more area under |f| for the same bound).
        for &k in &[1.0, 2.4, 8.0, 32.0] {
            let f = knee(k);
            assert!((f(1e-4) / 1e-4 - 1.0).abs() < 1e-3, "unity slope at k={k}");
            assert!(f(50.0) <= 1.0 + 1e-9 && f(50.0) > 0.9, "bounded at k={k}");
            assert!((f(-2.0) + f(2.0)).abs() < 1e-12, "odd symmetry at k={k}");
        }
        assert!(
            knee(32.0)(1.2) > knee(1.0)(1.2),
            "harder knee holds level longer"
        );
    }

    #[test]
    fn asym_clips_each_polarity_at_its_own_ceiling() {
        let f = asym(0.9, 0.4, 8.0);
        assert!(f(10.0) < 0.91 && f(10.0) > 0.85);
        assert!(f(-10.0) > -0.41 && f(-10.0) < -0.35);
        assert!((f(1e-4) / 1e-4 - 1.0).abs() < 1e-3, "unity slope at 0");
    }

    #[test]
    fn fold_folds() {
        // depth 0: monotonic on the primary lobe; depth 0.5: non-monotonic
        // (that's the point).
        let f0 = fold(0.0);
        assert!(f0(0.9) > f0(0.5));
        let f5 = fold(0.5);
        let mut prev = f5(0.0);
        let mut folded = false;
        for i in 1..100 {
            let y = f5(i as f64 / 50.0);
            if y < prev - 1e-6 {
                folded = true;
                break;
            }
            prev = y;
        }
        assert!(folded, "depth 0.5 should fold");
    }

    #[test]
    fn signed_chebyshev_silence_is_silence() {
        let f = chebyshev([1.0, 0.3, -0.2, 0.1, -0.25]);
        assert!(f(0.0).abs() < 1e-12);
        // Negative weights change the shape vs their positive twins.
        let g = chebyshev([1.0, 0.3, 0.2, 0.1, 0.25]);
        assert!((f(0.7) - g(0.7)).abs() > 1e-3);
    }

    #[test]
    fn composition_stacks() {
        // drive → asym clip → mild fold, as one function.
        let composite = compose(pre_gain(3.0, asym(1.0, 0.5, 6.0)), fold(0.1));
        for i in 0..200 {
            let x = -2.0 + 4.0 * i as f64 / 199.0;
            assert!(composite(x).is_finite());
        }
        assert!(composite(0.0).abs() < 1e-9);
    }
}
