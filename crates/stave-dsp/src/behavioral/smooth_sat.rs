//! Smooth saturator — tube-like warmth with a controllable, fizz-free top
//! end and low intermodulation.
//!
//! Two ideas, neither possible in a single analog circuit:
//!
//!  1. **Chebyshev harmonic control.** Because `T_n(cos θ) = cos(n θ)`, a
//!     weighted sum `Σ aₙ·Tₙ(x)` generates a *directly specified* harmonic
//!     spectrum: `a₂` is the 2nd-harmonic amount, `a₃` the 3rd, and the
//!     high orders are rolled off by a `smoothness` control. Low-order
//!     dominance = tube warmth; suppressed high orders = no hairy fizz,
//!     and far less intermodulation (low-order nonlinearities produce
//!     fewer intermod products than high-order ones).
//!
//!  2. **Companding around the shaper.** A high-shelf *cut* before the
//!     shaper and a complementary *boost* after it: the highs go in quiet
//!     (so they generate little distortion / IMD) and come out at full
//!     level. The frequency response stays flat — it's not a filter on the
//!     sound, it's a filter on the *distortion*. This is the natural
//!     mechanism (Miller capacitance / tape HF loss) generalised.
//!
//! Amplitude-dependence is built in: for an input below full scale the
//! Chebyshev cross-terms shrink, so quiet passages stay clean and the
//! specified spectrum is reached only as the signal approaches full level
//! — i.e. it saturates like real gear. Memoryless (per-sample), so run it
//! inside an oversampler for anti-aliasing.

use crate::behavioral::shapes::soft_clip;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};

/// Number of Chebyshev terms (harmonic orders modelled).
const N: usize = 10;

// The shaper's bound is `soft_clip` (tanh): it keeps the Chebyshev
// argument strictly inside (−1, 1) for stability while rounding peaks
// instead of flat-topping (an identity-then-exp clip flat-topped hard at
// high drive and left a slowly decaying high-harmonic tail — fizz); the
// Chebyshev sum then adds the tunable even/odd colour on top.

/// A saturator whose harmonic spectrum is specified directly, between
/// complementary high shelves that keep its distortion off the highs.
///
/// # Models
///
/// A designed curve with no hardware claim: `Σ aₙ·Tₙ(tanh(drive·x + bias))`
/// over ten Chebyshev orders, inside a −/+ `hf_db` high-shelf pair at
/// 2.5 kHz and a 5 Hz DC blocker.
///
/// # Derivation
///
/// The Chebyshev identity `Tₙ(cos θ) = cos(nθ)` (each weight is one
/// harmonic's amount), the small-signal slope pinned to unity, and
/// pre/de-emphasis companding as in tape and broadcast practice.
///
/// # Verified
///
/// `crates/stave-dsp/src/behavioral/smooth_sat/tests.rs::harmonic_profile_is_warm_and_smooth`
/// (2nd harmonic above −45 dB, 6th at least 12 dB under it, smoothness
/// cutting the 4th and 6th),
/// `crates/stave-dsp/src/behavioral/smooth_sat/tests.rs::companding_reduces_intermod`
/// (the 1 kHz difference tone of 10 and 11 kHz at least 12 dB lower
/// with companding) and
/// `crates/stave-dsp/src/behavioral/smooth_sat/tests.rs::oversampled_alias_floor_high_input`
/// (a 7 kHz tone's aliases below −65 dB at 4×);
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`
/// (DSP.md R5): at drive 1.6 with the shaper in a 4× region, a
/// stepped tone's inharmonic energy below 20 kHz under −101 dB at
/// 44.1 kHz (measured −103.3) and −97 at 48 (−99.9).
///
/// # Cost
///
/// No datasheet line yet (DSP.md P1): three biquads, a `tanh` and a
/// ten-term recurrence per sample.
///
/// # Aliasing
///
/// None in the [`Stage`] view, which runs everything at the base rate
/// (for analysis). Products run [`tick_pre`](Self::tick_pre), then
/// [`shape`](Self::shape) inside an oversampled region (4× for the
/// budget above), then [`tick_post`](Self::tick_post).
///
/// # Valid ranges
///
/// `even` and `odd` in 0–1, smoothness in 0–1 (exclusive), `hf_db ≥ 0`
/// (debug-asserted); `drive ≥ 0` (clamped). The tanh keeps the
/// Chebyshev argument inside (−1, 1) at any drive.
#[derive(Clone)]
pub struct SmoothSat {
    a: [f32; N],    // harmonic weights: a[0]=a₁ (fundamental) .. a[N-1]=a_N
    dc_offset: f32, // Σ aₙ·Tₙ(0) — subtracted so f(0)=0 (no zero-input DC)
    drive: f32,
    bias: f32,    // operating-point offset on the shaper (asymmetry / even harmonics)
    bias_dc: f32, // shaper value at the bias point — subtracted so f(0)=0 even with bias
    even: f32,
    odd: f32,
    smoothness: f32,
    hf_db: f32,
    pre: Biquad,  // de-emphasis (HF cut into the shaper)
    post: Biquad, // re-emphasis (complementary HF boost)
    dc: Biquad,   // remove DC from even-order terms
}

impl Default for SmoothSat {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothSat {
    pub fn new() -> Self {
        let mut s = Self {
            a: [0.0; N],
            dc_offset: 0.0,
            drive: 1.0,
            bias: 0.0,
            bias_dc: 0.0,
            even: 0.15,
            odd: 0.10,
            smoothness: 0.7,
            hf_db: 4.0,
            pre: Biquad::passthrough(),
            post: Biquad::passthrough(),
            dc: Biquad::new(FilterSpec::Highpass { hz: 5.0, q: 0.707 }),
        };
        s.rebuild_weights();
        s
    }

    /// Input gain into the shaper (how hard / how much of the spectrum is
    /// reached). Higher = more saturation, level-dependently.
    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.max(0.0);
    }
    /// Operating-point offset on the shaper — shifts where the signal sits
    /// on the curve, making it asymmetric (even-harmonic / "tube bias"
    /// colour). 0 = symmetric. The induced DC is removed by the output DC
    /// blocker. Cheap (scalar) so it can be modulated per sample, e.g. from
    /// a slow tube-bias model.
    pub fn set_bias(&mut self, bias: f32) {
        self.bias = bias;
        // cache the operating-point value so the shaper stays f(0)=0
        self.bias_dc = self.chebyshev(soft_clip(bias));
    }
    /// 2nd-harmonic (even) weight — tube-style warmth.
    pub fn set_even(&mut self, a: f32) {
        debug_assert!((0.0..=1.0).contains(&a), "even weight {a}");
        self.even = a;
        self.rebuild_weights();
    }
    /// 3rd-harmonic (odd) weight — body / edge.
    pub fn set_odd(&mut self, a: f32) {
        debug_assert!((0.0..=1.0).contains(&a), "odd weight {a}");
        self.odd = a;
        self.rebuild_weights();
    }
    /// High-order harmonic roll-off, 0..1. 0 = bright/buzzy (high orders
    /// present), →1 = silky (only 2nd/3rd survive). This is the
    /// fizz-control knob.
    pub fn set_smoothness(&mut self, s: f32) {
        debug_assert!((0.0..1.0).contains(&s), "smoothness {s}");
        self.smoothness = s;
        self.rebuild_weights();
    }
    /// Companding depth (dB) — how much the highs are de-/re-emphasised
    /// around the shaper. More = less HF distortion / IMD, same brightness.
    pub fn set_hf_smoothing(&mut self, db: f32) {
        debug_assert!(db >= 0.0, "companding depth {db} dB");
        // Only recompute the shelf coefficients on an actual change — the
        // plugin calls this every block.
        if (db - self.hf_db).abs() > 1e-4 {
            self.hf_db = db;
            self.update_companding();
        }
    }

    fn rebuild_weights(&mut self) {
        if N > 1 {
            self.a[1] = self.even; // 2nd
        }
        if N > 2 {
            self.a[2] = self.odd; // 3rd
        }
        // High orders: alternate even/odd seed, decayed by smoothness.
        // The decay base is FLOORED at DECAY_MAX < 1: a base of 1.0 would
        // be a flat, undecaying harmonic ladder (h10 as loud as h4) — a
        // shape no physical device produces, whose transfer curve goes
        // non-monotonic (locally inverting) and whose low-mid harmonic
        // budget cliffs. 0.7 ≈ −3 dB/order at smoothness 0: denser than a
        // hard clipper's 1/n tail (still has real teeth) but always a
        // decaying series, and the transfer stays monotonic everywhere.
        const DECAY_MAX: f32 = 0.7;
        for n in 4..=N {
            let seed = if n % 2 == 0 { self.even } else { self.odd };
            let decay = (DECAY_MAX * (1.0 - self.smoothness)).powi((n - 3) as i32);
            self.a[n - 1] = seed * decay;
        }
        // === Small-signal slope normalisation (unity below the knee) ===
        // The ODD weights contribute to the slope at 0: Tₙ'(0) =
        // n·sin(nπ/2) = ±n, so f'(0) = a₁ − 3a₃ + 5a₅ − 7a₇ + 9a₉. Left
        // unnormalised, changing odd/smoothness re-gains QUIET content by
        // several dB (loud content is pinned by the tanh bound), which the
        // ear reads as a low-pass sweeping with the smoothness knob — the
        // "kills natural top end" complaint. Pin f'(0) = 1 exactly:
        // quiet signals and small HF components always pass at unity; the
        // knobs then shape only what saturation does to LOUD content.
        let mut c = 0.0f32; // slope contribution of odd orders ≥ 3
        let mut n = 3;
        while n <= N {
            let tp = if n % 4 == 1 { n as f32 } else { -(n as f32) }; // Tₙ'(0)
            c += self.a[n - 1] * tp;
            n += 2;
        }
        // a₁ = 1 − c keeps f'(0) = 1. Guard the pathological corner (huge
        // odd weight, no decay ⇒ c ≥ 1 would drive a₁ ≤ 0 and INVERT quiet
        // signal — previously reachable at max odd + low… high smoothness):
        // floor a₁ and shrink the odd tail to fit instead.
        const A1_MIN: f32 = 0.35;
        if c > 1.0 - A1_MIN {
            let k = (1.0 - A1_MIN) / c;
            let mut n = 3;
            while n <= N {
                self.a[n - 1] *= k;
                n += 2;
            }
            self.a[0] = A1_MIN;
        } else {
            self.a[0] = 1.0 - c;
        }
        // Tₙ(0) = 0 for odd n, (−1)^(n/2) for even n. Sum the static DC the
        // even terms inject at zero input so we can null it (silence→silence).
        let mut off = 0.0f32;
        let mut n = 2;
        while n <= N {
            let sign = if (n / 2) % 2 == 0 { 1.0 } else { -1.0 };
            off += self.a[n - 1] * sign;
            n += 2;
        }
        self.dc_offset = off;
        // Weights changed ⇒ the operating-point value moved; keep it cached.
        self.bias_dc = self.chebyshev(soft_clip(self.bias));
    }

    fn update_companding(&mut self) {
        // Complementary high shelves at ~2.5 kHz: cut in, boost out.
        // IMPORTANT: do NOT reset the filter state here — this is called
        // from the per-block param update, and zeroing the shelf state
        // every buffer is a discontinuity each block = zipper noise on the
        // top end. State is only cleared in prepare()/reset().
        self.pre.set(FilterSpec::HighShelf {
            hz: 2_500.0,
            db: -self.hf_db,
        });
        self.post.set(FilterSpec::HighShelf {
            hz: 2_500.0,
            db: self.hf_db,
        });
    }

    /// Σ aₙ·Tₙ(x) via the Chebyshev recurrence (x must be in [-1, 1]).
    #[inline]
    fn chebyshev(&self, x: f32) -> f32 {
        let mut t0 = 1.0f32; // T₀
        let mut t1 = x; // T₁
        let mut y = self.a[0] * t1;
        let mut n = 2;
        while n <= N {
            let t2 = 2.0 * x * t1 - t0;
            y += self.a[n - 1] * t2;
            t0 = t1;
            t1 = t2;
            n += 1;
        }
        y - self.dc_offset
    }

    /// De-emphasis (drive + HF cut) — BASE RATE, runs before upsampling.
    #[inline]
    pub fn tick_pre(&mut self, x: f32) -> f32 {
        self.pre.tick(x * self.drive)
    }

    /// The memoryless shaper — runs INSIDE the oversampler. Offsets by the
    /// operating-point `bias`, then a smooth soft-clip (identity below 0.95,
    /// C1 tanh round in the top 5 %) bounds the Chebyshev argument without
    /// hard-clipping (so driving hard rounds gently, no clip fizz), then
    /// `Σ aₙ·Tₙ(x)`. With bias = 0 this is exactly the symmetric curve.
    #[inline]
    pub fn shape(&self, x: f32) -> f32 {
        self.chebyshev(soft_clip(x + self.bias)) - self.bias_dc
    }

    /// Re-emphasis (HF boost) + DC block — BASE RATE, after decimation.
    #[inline]
    pub fn tick_post(&mut self, x: f32) -> f32 {
        self.dc.tick(self.post.tick(x))
    }

    /// Unification bridge: export the CURRENT recipe (weights,
    /// slope normalisation, tanh bound — everything `shape` computes) as
    /// a tabulated [`CurveStage`](crate::behavioral::curve::CurveStage) for
    /// Chain-based consumers. The stage gains ADAA and morphability
    /// (`Curve::combine`); bias becomes the frame offset. SmoothSat
    /// itself remains the analytic base-rate-companded engine for its
    /// existing products.
    pub fn to_curve_stage(&self) -> crate::behavioral::curve::CurveStage {
        crate::behavioral::curve::CurveStage::from_fn(-6.0, 6.0, 0.0, |x| {
            self.shape(x as f32) as f64
        })
    }
}

impl Stage for SmoothSat {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.pre.prepare(sample_rate);
        self.post.prepare(sample_rate);
        self.dc.prepare(sample_rate);
        self.update_companding();
        self.reset();
    }

    fn reset(&mut self) {
        self.pre.reset();
        self.post.reset();
        self.dc.reset();
    }

    /// Base-rate convenience (no oversampling) — the offline
    /// harmonic/IMD analysis. Oversampled use runs
    /// `tick_pre` → 2×(`shape`) → `tick_post`, so the stateful filters
    /// stay at base rate and only the memoryless shaper is oversampled.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let pre = self.tick_pre(x);
        let shaped = self.shape(pre);
        self.tick_post(shaped)
    }

    fn tail(&self) -> Tail {
        self.pre.tail().then(self.post.tail()).then(self.dc.tail())
    }
}

#[cfg(test)]
mod tests;
