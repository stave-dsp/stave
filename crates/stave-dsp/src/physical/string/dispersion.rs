//! Stiffness: a cascade of identical first-order allpass sections in
//! the string's loop, and its design from the inharmonicity `B`.

/// The most allpass sections a string's dispersion filter may use.
pub const MAX_SECTIONS: usize = 16;

/// The phase lag (radians) of `H(z) = (a + z⁻¹)/(1 + a z⁻¹)` at `w`
/// radians per sample: 0 at DC, π at Nyquist, continuous for |a| < 1.
#[inline]
pub(super) fn allpass_lag(a: f64, w: f64) -> f64 {
    let (s, c) = w.sin_cos();
    (-a * s).atan2(1.0 + a * c) - (-s).atan2(a + c)
}

/// Its group delay, samples: `(1 − a²)/(1 + 2a cos w + a²)`.
#[inline]
pub(super) fn allpass_group(a: f64, w: f64) -> f64 {
    (1.0 - a * a) / (1.0 + 2.0 * a * w.cos() + a * a)
}

/// The phase lag of the loss filter `g(1 − p)/(1 − p z⁻¹)`.
#[inline]
pub(super) fn one_pole_lag(p: f64, w: f64) -> f64 {
    let (s, c) = w.sin_cos();
    (p * s).atan2(1.0 - p * c)
}

/// Its group delay, samples.
#[inline]
pub(super) fn one_pole_group(p: f64, w: f64) -> f64 {
    let c = w.cos();
    (p * c - p * p) / (1.0 - 2.0 * p * c + p * p)
}

/// A string's dispersion filter: `sections` identical first-order
/// allpasses of coefficient `coefficient` (negative: low frequencies
/// are delayed more, so the upper partials run sharp, as a stiff
/// string's do).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Dispersion {
    pub sections: usize,
    pub coefficient: f64,
}

impl Dispersion {
    /// None: a flexible string, harmonic.
    pub const NONE: Self = Self { sections: 0, coefficient: 0.0 };

    /// Rauhala & Välimäki's closed form (DAFx-06, eqs. 3–8 with their
    /// table 2): the Thiran delay `D` per section from the key index of
    /// `f0` (the ideal string's fundamental, Hz), `B` and the section
    /// count, and `a = (1 − D)/(1 + D)`. Fitted by the authors at a
    /// 44.1 kHz rate; [`fit`](Self::fit) refines it for any rate.
    pub fn rauhala(f0: f64, b: f64, sections: usize) -> Self {
        if b <= 0.0 || sections == 0 {
            return Self::NONE;
        }
        let (k1, k2, k3) = (-0.00179, -0.0233, -2.93);
        let (m1, m2, m3, m4) = (0.0126, 0.0606, -0.00825, 1.97);
        let ln_b = b.ln();
        let ln_m = (sections as f64).ln();
        let key = 12.0 * (f0 / 27.5).log2() + 1.0;
        let kd = (k1 * ln_b * ln_b + k2 * ln_b + k3).exp();
        let cd = ((m1 * ln_m + m2) * ln_b + m3 * ln_m + m4).exp();
        let d = (cd - key * kd).exp().max(1.0);
        Self { sections: sections.min(MAX_SECTIONS), coefficient: (1.0 - d) / (1.0 + d) }
    }

    /// The coefficient that puts partials 2…`partials` nearest (least
    /// squares, in cents) to `f_k = k f0 √(1 + B k²)`, with the loop's
    /// line length set so partial 1 is exact. `w0 = 2π f0/fs`;
    /// `loss_lag(w)` and `loss_group(w)` describe the rest of the loop
    /// (the loss filter); `shortest` is the least line length (samples)
    /// the loop may be left with. Golden-section search on
    /// `a ∈ [−0.98, 0]`: 48 steps, no allocation.
    pub fn fit(
        w0: f64,
        b: f64,
        sections: usize,
        partials: usize,
        shortest: f64,
        loss_lag: impl Fn(f64) -> f64,
        loss_group: impl Fn(f64) -> f64,
    ) -> Self {
        let sections = sections.min(MAX_SECTIONS);
        if b <= 0.0 || sections == 0 || partials < 2 {
            return Self::NONE;
        }
        let m = sections as f64;
        let wk = |k: f64| k * w0 * (1.0 + b * k * k).sqrt();
        let w1 = wk(1.0);
        let cost = |a: f64| -> f64 {
            let line = (core::f64::consts::TAU - loss_lag(w1) - m * allpass_lag(a, w1)) / w1;
            if line < shortest {
                return 1e12 * (1.0 + shortest - line);
            }
            let mut sum = 0.0;
            for k in 2..=partials {
                let w = wk(k as f64);
                let err = w * line + loss_lag(w) + m * allpass_lag(a, w) - core::f64::consts::TAU * k as f64;
                let group = line + loss_group(w) + m * allpass_group(a, w);
                let cents = 1200.0 * (1.0 - err / (group * w)).max(1e-9).log2();
                sum += cents * cents;
            }
            sum
        };
        let phi = 0.5 * (5f64.sqrt() - 1.0);
        let (mut lo, mut hi) = (-0.98f64, 0.0f64);
        let mut x1 = hi - phi * (hi - lo);
        let mut x2 = lo + phi * (hi - lo);
        let (mut f1, mut f2) = (cost(x1), cost(x2));
        for _ in 0..48 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - phi * (hi - lo);
                f1 = cost(x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + phi * (hi - lo);
                f2 = cost(x2);
            }
        }
        Self { sections, coefficient: 0.5 * (lo + hi) }
    }

    /// The filter's phase lag at `w`.
    pub fn lag(&self, w: f64) -> f64 {
        self.sections as f64 * allpass_lag(self.coefficient, w)
    }

    /// The filter's group delay at `w`, samples.
    pub fn group(&self, w: f64) -> f64 {
        self.sections as f64 * allpass_group(self.coefficient, w)
    }
}
