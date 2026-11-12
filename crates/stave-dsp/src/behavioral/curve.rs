//! Tabulated 1-D transfer function with antiderivatives — the shared
//! representation every memoryless nonlinearity compiles down to.
//!
//! A `Curve` stores a transfer `f` sampled on a uniform grid over
//! `[x_min, x_max]`, together with its first and second antiderivatives
//! `g1 = ∫f dx` and `g2 = ∫g1 dx` (built by trapezoid integration and
//! referenced to 0 at a caller-chosen rest point `ref_x`, so the ADAA
//! difference-quotient operands stay small and well-conditioned).
//!
//! Design rules, inherited from the triode implementation this was
//! extracted from (and from the LUT-ADAA literature — Gabrielli/Squartini,
//! DAFx 2025):
//!
//!  - **Catmull-Rom (cubic) interpolation, not linear.** Linear
//!    interpolation's piecewise-constant slope puts kinks in the
//!    antiderivatives; the kinks inject spurious odd harmonics that swamp
//!    a curve's genuine low-level harmonic content. C1 continuity removes
//!    the artifact.
//!  - **Physically-correct edge extrapolation.** Past the tabulated range
//!    `f` is held constant, so `g1` continues *linearly* and `g2`
//!    *quadratically*. Clamping the integrals instead would corrupt the
//!    difference quotients for any excursion past the table (e.g. deep
//!    cutoff at high drive).
//!  - **Parameters deform the curve in three tiers.**
//!      1. *Affine* (bias, drive, output gain) — applied outside the table
//!         via [`Curve::framed`]; free, safe to move per sample (slowly).
//!      2. *Linear-in-parameter* (e.g. Chebyshev harmonic weights) —
//!         [`Curve::combine`] rebuilds by weighted sum of basis tables;
//!         integration is linear, so the antiderivatives combine exactly.
//!         No solver, no allocation — block-rate safe.
//!      3. *Structural* (e.g. a different tube's Koren constants) — full
//!         [`Curve::rebuild`] from the source function. Rare; guard it.
//!
//! The affine frame is the contract with program-dependent "memory":
//! `Modulation { bias, drive, gain }` maps 1:1 onto `(offset, in_scale,
//! out_scale)`. ADAA assumes the curve is frozen within one difference
//! quotient — commit frame changes *between* samples, never mid-evaluation
//! (see `Adaa` docs).
//!
//! Storage is inline fixed-size (`CURVE_N` f64 per table) so a rebuild on
//! the audio thread (e.g. a tube-type switch) allocates nothing.

/// Table resolution. 1024 is well past the point where a Catmull-Rom
/// LUT is distinguishable from analytic ADAA (per the DAFx reference,
/// even ~128 is clean for smooth curves).
pub const CURVE_N: usize = 1024;

/// A tabulated transfer + antiderivatives. See module docs.
extern crate alloc;

/// The tabulated columns, heap-allocated ONCE at construction: a Curve is
/// ~32 KB of tables, and stages/chains embed several — inline arrays blew
/// test-thread stacks during by-value construction. Rebuilds write in
/// place (no audio-thread allocation after `flat()`/`from_fn`).
#[derive(Clone)]
struct Tables {
    f: [f64; CURVE_N],
    g1: [f64; CURVE_N],
    g2: [f64; CURVE_N],
    /// Optional auxiliary SENSE column:
    /// a physical observable of the same input (plate current, diode
    /// conduction, grid excess), tabulated alongside the transfer so
    /// modulation drivers can key on physics instead of raw level.
    sense: [f64; CURVE_N],
}

pub struct Curve {
    /// SHARED tables (the perf deep-dive, 2026-08-05): a curve's
    /// tables are immutable between rebuilds, and hot product models
    /// clone curves per stage — Arc turns those clones into
    /// refcount bumps so identical stages READ THE SAME MEMORY.
    /// Measured: ~40 boxed 32 KB table sets per lane cycled every
    /// sample = every interp an L2 round-trip (~36 ns/iteration in
    /// the TS solve). Rebuild copies-on-write (bind-rate, same
    /// alloc class as the Box it replaces).
    t: alloc::sync::Arc<Tables>,
    has_sense: bool,
    x_min: f64,
    x_max: f64,
    inv_step: f64,
}

impl Clone for Curve {
    fn clone(&self) -> Self {
        // Refcount bump: clones SHARE tables until one rebuilds
        // (copy-on-write in `tables_mut`). Identical stages read
        // identical memory — the cache is the point.
        Self {
            t: alloc::sync::Arc::clone(&self.t),
            has_sense: self.has_sense,
            x_min: self.x_min,
            x_max: self.x_max,
            inv_step: self.inv_step,
        }
    }
}

impl Default for Curve {
    fn default() -> Self {
        Self::flat()
    }
}

/// Read access to a shaped transfer and its antiderivatives, as consumed
/// by `Adaa`. `f1`/`f2` must be exact antiderivatives of `f` (up to an
/// additive constant) or the difference quotients are meaningless.
pub trait AdaaShape {
    fn f(&self, x: f64) -> f64;
    fn f1(&self, x: f64) -> f64;
    fn f2(&self, x: f64) -> f64;
}

impl Curve {
    /// An identity-silent curve (f ≡ 0 over [−1, 1]). Placeholder until
    /// `rebuild`/`combine` is called. Allocates the table block (the ONLY
    /// allocation a Curve ever makes — construction time, never audio).
    pub fn flat() -> Self {
        Self {
            t: alloc::sync::Arc::new(Tables {
                f: [0.0; CURVE_N],
                g1: [0.0; CURVE_N],
                g2: [0.0; CURVE_N],
                sense: [0.0; CURVE_N],
            }),
            has_sense: false,
            x_min: -1.0,
            x_max: 1.0,
            inv_step: (CURVE_N - 1) as f64 / 2.0,
        }
    }

    /// Build from a source function. In-place, no allocation — safe on the
    /// audio thread if `source` is (the cost is CURVE_N evaluations plus
    /// two trapezoid passes). `ref_x` is where the signal rests (e.g. the
    /// quiescent bias): both antiderivatives are referenced to 0 there.
    pub fn rebuild(
        &mut self,
        x_min: f64,
        x_max: f64,
        ref_x: f64,
        mut source: impl FnMut(f64) -> f64,
    ) {
        debug_assert!(x_max > x_min);
        self.x_min = x_min;
        self.x_max = x_max;
        let step = (x_max - x_min) / ((CURVE_N - 1) as f64);
        self.inv_step = 1.0 / step;
        let t = alloc::sync::Arc::make_mut(&mut self.t);
        for k in 0..CURVE_N {
            t.f[k] = source(x_min + k as f64 * step);
        }
        self.has_sense = false;
        self.integrate(ref_x, step);
    }

    /// Like [`Curve::rebuild`], additionally tabulating a sense column
    /// (an observable of the same input) for modulation drivers.
    pub fn rebuild_with_sense(
        &mut self,
        x_min: f64,
        x_max: f64,
        ref_x: f64,
        mut source: impl FnMut(f64) -> f64,
        mut sense: impl FnMut(f64) -> f64,
    ) {
        self.rebuild(x_min, x_max, ref_x, &mut source);
        let step = (x_max - x_min) / ((CURVE_N - 1) as f64);
        let t = alloc::sync::Arc::make_mut(&mut self.t);
        for k in 0..CURVE_N {
            t.sense[k] = sense(x_min + k as f64 * step);
        }
        self.has_sense = true;
    }

    /// Sense column at `x` (constant-extrapolated); 0 if none tabulated.
    #[inline]
    pub fn sense_at(&self, x: f64) -> f64 {
        if !self.has_sense {
            return 0.0;
        }
        if x <= self.x_min {
            self.t.sense[0]
        } else if x >= self.x_max {
            self.t.sense[CURVE_N - 1]
        } else {
            interp(&self.t.sense, self.t_of(x))
        }
    }
    pub fn has_sense(&self) -> bool {
        self.has_sense
    }

    /// Rebuild as a weighted sum of basis curves: `f = Σ wᵢ·fᵢ`. Because
    /// integration is linear, the antiderivatives are re-integrated from
    /// the combined `f` (equivalently Σ wᵢ·g1ᵢ, but re-integrating keeps a
    /// single referencing path). All parts must share this curve's target
    /// domain; the first part supplies it.
    ///
    /// This is the "linear-in-parameter" tier: weight changes cost one
    /// combine, no source evaluations, no allocation.
    pub fn combine(&mut self, ref_x: f64, parts: &[(&Curve, f64)]) {
        let Some(&(first, _)) = parts.first() else {
            return;
        };
        self.x_min = first.x_min;
        self.x_max = first.x_max;
        let step = (self.x_max - self.x_min) / ((CURVE_N - 1) as f64);
        self.inv_step = 1.0 / step;
        for k in 0..CURVE_N {
            let mut acc = 0.0;
            for &(c, w) in parts {
                debug_assert!(
                    c.x_min == self.x_min && c.x_max == self.x_max,
                    "combine: basis domains must match"
                );
                acc += w * c.t.f[k];
            }
            alloc::sync::Arc::make_mut(&mut self.t).f[k] = acc;
        }
        self.integrate(ref_x, step);
    }

    /// Trapezoid-integrate `f` into `g1`, `g1` into `g2`, then reference
    /// both to 0 at `ref_x`.
    fn integrate(&mut self, ref_x: f64, step: f64) {
        let t_ref = self.t_of(ref_x);
        let t = alloc::sync::Arc::make_mut(&mut self.t);
        t.g1[0] = 0.0;
        for k in 1..CURVE_N {
            t.g1[k] = t.g1[k - 1] + 0.5 * (t.f[k] + t.f[k - 1]) * step;
        }
        let off1 = interp(&t.g1, t_ref);
        for v in t.g1.iter_mut() {
            *v -= off1;
        }
        t.g2[0] = 0.0;
        for k in 1..CURVE_N {
            t.g2[k] = t.g2[k - 1] + 0.5 * (t.g1[k] + t.g1[k - 1]) * step;
        }
        let off2 = interp(&t.g2, t_ref);
        for v in t.g2.iter_mut() {
            *v -= off2;
        }
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.x_min, self.x_max)
    }

    /// Grid coordinate (fractional index) of a domain point, clamped.
    #[inline]
    fn t_of(&self, x: f64) -> f64 {
        ((x - self.x_min) * self.inv_step).clamp(0.0, (CURVE_N - 1) as f64)
    }

    /// Transfer at `x`, constant-extrapolated past the domain.
    #[inline]
    pub fn f_at(&self, x: f64) -> f64 {
        if x <= self.x_min {
            self.t.f[0]
        } else if x >= self.x_max {
            self.t.f[CURVE_N - 1]
        } else {
            interp(&self.t.f, self.t_of(x))
        }
    }

    /// Transfer AND its derivative at `x`, from one table fetch: the
    /// slope is the ANALYTIC derivative of the same Catmull-Rom
    /// segment `f_at` evaluates — the true df/dx of the interpolated
    /// curve (a finite difference approximates this with three full
    /// evaluations and an H of its own; Newton solvers were paying
    /// 3× for a worse answer — 22% of forge's screamer-mode block
    /// cost, sampled). Outside the domain the value is constant and
    /// the slope is 0.
    #[inline]
    pub fn f_and_slope_at(&self, x: f64) -> (f64, f64) {
        if x <= self.x_min {
            (self.t.f[0], 0.0)
        } else if x >= self.x_max {
            (self.t.f[CURVE_N - 1], 0.0)
        } else {
            let t = self.t_of(x);
            let (f, df_dt) = interp_with_slope(&self.t.f, t);
            (f, df_dt * self.inv_step)
        }
    }

    /// First antiderivative, linearly extrapolated (since `f` is constant
    /// past the edges).
    #[inline]
    pub fn g1_at(&self, x: f64) -> f64 {
        if x < self.x_min {
            self.t.g1[0] + self.t.f[0] * (x - self.x_min)
        } else if x > self.x_max {
            self.t.g1[CURVE_N - 1] + self.t.f[CURVE_N - 1] * (x - self.x_max)
        } else {
            interp(&self.t.g1, self.t_of(x))
        }
    }

    /// Second antiderivative, quadratically extrapolated.
    #[inline]
    pub fn g2_at(&self, x: f64) -> f64 {
        if x < self.x_min {
            let d = x - self.x_min;
            self.t.g2[0] + self.t.g1[0] * d + 0.5 * self.t.f[0] * d * d
        } else if x > self.x_max {
            let d = x - self.x_max;
            self.t.g2[CURVE_N - 1]
                + self.t.g1[CURVE_N - 1] * d
                + 0.5 * self.t.f[CURVE_N - 1] * d * d
        } else {
            interp(&self.t.g2, self.t_of(x))
        }
    }

    /// View the curve through an affine frame: the shaped transfer is
    /// `F(x) = out_scale · f(offset + in_scale·x)`. The scaling factors
    /// out of the integrals exactly:
    /// `F1(x) = (out_scale/in_scale)·g1(offset + in_scale·x)` and
    /// `F2(x) = (out_scale/in_scale²)·g2(…)`, so ADAA works at any frame
    /// without touching the tables.
    #[inline]
    pub fn framed(&self, offset: f64, in_scale: f64, out_scale: f64) -> Framed<'_> {
        debug_assert!(in_scale != 0.0);
        Framed {
            curve: self,
            offset,
            in_scale,
            out_scale,
        }
    }
}

/// Catmull-Rom (cubic) interpolation at fractional index `t` (in range).
/// Edge control points are clamped.
#[inline]
fn interp(table: &[f64; CURVE_N], t: f64) -> f64 {
    let i = (t as usize).min(CURVE_N - 1);
    let frac = t - i as f64;
    let p0 = table[i.saturating_sub(1)];
    let p1 = table[i];
    let p2 = table[(i + 1).min(CURVE_N - 1)];
    let p3 = table[(i + 2).min(CURVE_N - 1)];
    let f2 = frac * frac;
    let f3 = f2 * frac;
    0.5 * (2.0 * p1
        + (-p0 + p2) * frac
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f3)
}

/// Catmull-Rom value + derivative w.r.t. `t` from one 4-point fetch.
fn interp_with_slope(table: &[f64; CURVE_N], t: f64) -> (f64, f64) {
    let i = (t as usize).min(CURVE_N - 1);
    let frac = t - i as f64;
    let p0 = table[i.saturating_sub(1)];
    let p1 = table[i];
    let p2 = table[(i + 1).min(CURVE_N - 1)];
    let p3 = table[(i + 2).min(CURVE_N - 1)];
    let c1 = -p0 + p2;
    let c2 = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
    let c3 = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let f2 = frac * frac;
    let value = 0.5 * (2.0 * p1 + c1 * frac + c2 * f2 + c3 * f2 * frac);
    let slope = 0.5 * (c1 + 2.0 * c2 * frac + 3.0 * c3 * f2);
    (value, slope)
}

/// A `Curve` seen through an affine frame — the object `Adaa` evaluates.
/// Cheap to construct per sample; holds no state.
#[derive(Clone, Copy)]
pub struct Framed<'a> {
    curve: &'a Curve,
    offset: f64,
    in_scale: f64,
    out_scale: f64,
}

impl AdaaShape for Framed<'_> {
    #[inline]
    fn f(&self, x: f64) -> f64 {
        self.out_scale * self.curve.f_at(self.offset + self.in_scale * x)
    }
    #[inline]
    fn f1(&self, x: f64) -> f64 {
        (self.out_scale / self.in_scale) * self.curve.g1_at(self.offset + self.in_scale * x)
    }
    #[inline]
    fn f2(&self, x: f64) -> f64 {
        (self.out_scale / (self.in_scale * self.in_scale))
            * self.curve.g2_at(self.offset + self.in_scale * x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Analytic reference: f = tanh has closed-form antiderivatives
    /// g1 = ln(cosh x), g2 = ∫ln(cosh) (checked by differentiating), so
    /// the table can be validated against ground truth, not just against
    /// itself.
    fn tanh_curve() -> Curve {
        let mut c = Curve::flat();
        c.rebuild(-8.0, 8.0, 0.0, |x| x.tanh());
        c
    }

    #[test]
    fn table_matches_source_function() {
        let c = tanh_curve();
        // Off-grid points (the interpolation, not the samples).
        // Catmull-Rom is O(h³)-accurate, h = 16/1023: bound ~1e-7 for a
        // curve this smooth (measured worst ≈ 5e-8).
        for i in 0..1000 {
            let x = -7.9 + 15.8 * (i as f64 + 0.371) / 1000.0;
            let err = (c.f_at(x) - x.tanh()).abs();
            assert!(err < 2e-7, "interp error {err:.2e} at x={x}");
        }
    }

    #[test]
    fn g1_matches_analytic_antiderivative() {
        // g1 referenced to 0 at ref_x=0; ln(cosh 0)=0, so they must agree
        // directly. Trapezoid over 1024 points: error O(step²) ≈ 6e-5 —
        // assert well under the level that matters for ADAA operands.
        let c = tanh_curve();
        for &x in &[-6.0f64, -2.5, -0.3, 0.0, 0.4, 1.7, 5.0] {
            let exact = x.cosh().ln();
            let err = (c.g1_at(x) - exact).abs();
            assert!(
                err < 5e-5,
                "g1 error {err:.2e} at x={x} ({} vs {exact})",
                c.g1_at(x)
            );
        }
    }

    #[test]
    fn antiderivatives_are_consistent() {
        // d/dx g1 ≈ f and d/dx g2 ≈ g1 by central differences — the
        // tables really are successive antiderivatives.
        // Tolerance: the ~1e-7 interpolation error is amplified by the
        // 1/(2h) = 500 of the central difference, plus O(h²) truncation —
        // budget ~1e-4.
        let c = tanh_curve();
        let h = 1e-3;
        for &x in &[-5.0, -1.2, 0.1, 0.9, 3.3] {
            let df1 = (c.g1_at(x + h) - c.g1_at(x - h)) / (2.0 * h);
            assert!((df1 - c.f_at(x)).abs() < 1e-4, "dg1 != f at {x}");
            let df2 = (c.g2_at(x + h) - c.g2_at(x - h)) / (2.0 * h);
            assert!((df2 - c.g1_at(x)).abs() < 1e-4, "dg2 != g1 at {x}");
        }
    }

    #[test]
    fn extrapolation_is_continuous_and_correct() {
        // Value and first derivative must be continuous across the table
        // edge, and the extrapolated antiderivatives must keep integrating
        // the constant edge value (linear g1, quadratic g2).
        let c = tanh_curve();
        let (lo, hi) = c.domain();
        let eps = 1e-9;
        for &(a, b) in &[(lo - eps, lo + eps), (hi - eps, hi + eps)] {
            assert!((c.f_at(a) - c.f_at(b)).abs() < 1e-6);
            assert!((c.g1_at(a) - c.g1_at(b)).abs() < 1e-6);
            assert!((c.g2_at(a) - c.g2_at(b)).abs() < 1e-6);
        }
        // 3 units past the edge: g1 must have advanced by 3·f_edge.
        let f_hi = c.f_at(hi);
        assert!((c.g1_at(hi + 3.0) - (c.g1_at(hi) + 3.0 * f_hi)).abs() < 1e-9);
        // and g2 by g1_edge·3 + f_edge·9/2.
        let expect = c.g2_at(hi) + c.g1_at(hi) * 3.0 + 0.5 * f_hi * 9.0;
        assert!((c.g2_at(hi + 3.0) - expect).abs() < 1e-9);
    }

    #[test]
    fn combine_is_exactly_linear() {
        // Σ wᵢ·basisᵢ combined must equal a direct rebuild of the weighted
        // sum — including the antiderivatives (integration is linear).
        let mut b1 = Curve::flat();
        b1.rebuild(-4.0, 4.0, 0.0, |x| x.tanh());
        let mut b2 = Curve::flat();
        b2.rebuild(-4.0, 4.0, 0.0, |x| (2.0 * x.tanh().powi(2)) - 1.0);

        let mut combined = Curve::flat();
        combined.combine(0.0, &[(&b1, 0.7), (&b2, 0.25)]);

        let mut direct = Curve::flat();
        direct.rebuild(-4.0, 4.0, 0.0, |x| {
            0.7 * x.tanh() + 0.25 * ((2.0 * x.tanh().powi(2)) - 1.0)
        });

        for i in 0..200 {
            let x = -3.9 + 7.8 * i as f64 / 199.0;
            assert!((combined.f_at(x) - direct.f_at(x)).abs() < 1e-12);
            assert!((combined.g1_at(x) - direct.g1_at(x)).abs() < 1e-9);
            assert!((combined.g2_at(x) - direct.g2_at(x)).abs() < 1e-9);
        }
    }

    #[test]
    fn framed_scaling_factors_out_of_integrals() {
        // F(x) = A·f(c + B·x) ⇒ F1 = (A/B)·g1(c+Bx), F2 = (A/B²)·g2(…).
        // Verify by differentiating the framed antiderivatives numerically.
        let c = tanh_curve();
        let fr = c.framed(-0.4, 2.5, 1.7);
        let h = 1e-4;
        for &x in &[-1.0, -0.2, 0.3, 1.1] {
            let df1 = (fr.f1(x + h) - fr.f1(x - h)) / (2.0 * h);
            assert!((df1 - fr.f(x)).abs() < 1e-4, "framed dF1 != F at {x}");
            let df2 = (fr.f2(x + h) - fr.f2(x - h)) / (2.0 * h);
            assert!((df2 - fr.f1(x)).abs() < 1e-4, "framed dF2 != F1 at {x}");
        }
    }

    #[test]
    fn reference_point_zeroes_antiderivatives() {
        let mut c = Curve::flat();
        c.rebuild(-8.0, 8.0, 1.3, |x| x.tanh());
        assert!(c.g1_at(1.3).abs() < 1e-9);
        assert!(c.g2_at(1.3).abs() < 1e-9);
    }
}

// CurveStage — the tabulated shape fitted with the node
// conventions (rate contract, ADAA ordering, Frame modulation
// surface). Raw math above; the Stage node below (one concept).
use crate::behavioral::adaa::{Adaa, AdaaOrder};
use crate::kernel::node::Stage;

/// The affine frame a modulator writes: input offset (operating point),
/// input scale (drive) and output scale (gain) on a [`CurveStage`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub bias: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Frame {
    pub const NEUTRAL: Frame = Frame {
        bias: 0.0,
        drive: 1.0,
        gain: 1.0,
    };
}

/// A tabulated memoryless nonlinearity with ADAA, whose affine frame is
/// modulation-safe by construction: writers touch `pending` at any time;
/// `tick` commits it BETWEEN difference quotients (the ordering ADAA
/// requires), so no caller can move the curve mid-evaluation.
#[derive(Clone)]
pub struct CurveStage {
    curve: Curve,
    adaa: Adaa,
    adaa_on: bool,
    frame: Frame,
    pending: Frame,
    last_x: f32,
}

impl CurveStage {
    /// Tabulate a shape over `[x_min, x_max]` (antiderivatives referenced
    /// at `ref_x`). Compose anything from `shapes` first — one table.
    pub fn from_fn(x_min: f64, x_max: f64, ref_x: f64, source: impl FnMut(f64) -> f64) -> Self {
        let mut curve = Curve::flat();
        curve.rebuild(x_min, x_max, ref_x, source);
        Self {
            curve,
            adaa: Adaa::new(AdaaOrder::First),
            adaa_on: true,
            frame: Frame::NEUTRAL,
            pending: Frame::NEUTRAL,
            last_x: 0.0,
        }
    }

    /// Tabulate with a SENSE column (physical observable — Step 2).
    pub fn from_fn_with_sense(
        x_min: f64,
        x_max: f64,
        ref_x: f64,
        source: impl FnMut(f64) -> f64,
        sense: impl FnMut(f64) -> f64,
    ) -> Self {
        let mut cs = Self::from_fn(x_min, x_max, ref_x, |_| 0.0);
        cs.curve
            .rebuild_with_sense(x_min, x_max, ref_x, source, sense);
        cs
    }

    /// Rebuild the shape in place (block rate; no allocation).
    pub fn rebuild(&mut self, x_min: f64, x_max: f64, ref_x: f64, source: impl FnMut(f64) -> f64) {
        self.curve.rebuild(x_min, x_max, ref_x, source);
    }

    /// Rebuild shape + sense column in place (block rate; no allocation).
    pub fn rebuild_with_sense(
        &mut self,
        x_min: f64,
        x_max: f64,
        ref_x: f64,
        source: impl FnMut(f64) -> f64,
        sense: impl FnMut(f64) -> f64,
    ) {
        self.curve
            .rebuild_with_sense(x_min, x_max, ref_x, source, sense);
    }

    /// Sense value at the LAST processed input, mapped through the
    /// committed frame — a physics detector for the ModMatrix (e.g. grid
    /// conduction driving blocking bias).
    #[inline]
    pub fn sense_tap(&self) -> f32 {
        self.curve
            .sense_at(self.frame.bias as f64 + self.frame.drive as f64 * self.last_x as f64)
            as f32
    }

    /// Modulation surface: written whenever, applied at the next tick.
    pub fn set_pending(&mut self, f: Frame) {
        self.pending = f;
    }
    pub fn pending_mut(&mut self) -> &mut Frame {
        &mut self.pending
    }
    pub fn set_adaa(&mut self, on: bool) {
        self.adaa_on = on;
    }
    pub fn curve(&self) -> &Curve {
        &self.curve
    }
}

impl Stage for CurveStage {
    fn prepare(&mut self, _sample_rate: f32) {
        self.reset();
    }
    fn reset(&mut self) {
        self.adaa.reset();
        self.frame = self.pending;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        // Commit the frame between difference quotients.
        self.frame = self.pending;
        let framed = self.curve.framed(
            self.frame.bias as f64,
            (self.frame.drive as f64).max(1e-6),
            self.frame.gain as f64,
        );
        self.last_x = x;
        let x0 = x as f64;
        let y = if self.adaa_on {
            self.adaa.tick(&framed, x0)
        } else {
            self.adaa.push(x0);
            use crate::behavioral::curve::AdaaShape;
            framed.f(x0)
        };
        y as f32
    }
}
