//! The linear blocks of a block-oriented model: cascaded second-order
//! sections (a rational filter) or FIR taps, at the rate the model was
//! fitted at.

use crate::kernel::node::Tail;
use crate::primitives::spectral::Complex;

/// One section `(b0 + b1·z⁻¹ + b2·z⁻²) / (1 + a1·z⁻¹ + a2·z⁻²)`; a
/// first-order section has `b2 = a2 = 0`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Section {
    pub b: [f64; 3],
    pub a: [f64; 2],
}

impl Section {
    /// `1`.
    pub const IDENTITY: Section = Section {
        b: [1.0, 0.0, 0.0],
        a: [0.0, 0.0],
    };

    /// The response at `w` radians per sample.
    pub fn response(&self, w: f64) -> Complex<f64> {
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let num = Complex::new(
            self.b[0] + self.b[1] * c1 + self.b[2] * c2,
            self.b[1] * s1 + self.b[2] * s2,
        );
        let den = Complex::new(1.0 + self.a[0] * c1 + self.a[1] * c2, self.a[0] * s1 + self.a[1] * s2);
        let d = den.norm_sqr();
        Complex::new(
            (num.re * den.re + num.im * den.im) / d,
            (num.im * den.re - num.re * den.im) / d,
        )
    }

    /// Both poles strictly inside the unit circle (the stability
    /// triangle `|a2| < 1`, `|a1| < 1 + a2`).
    pub fn is_stable(&self) -> bool {
        let [a1, a2] = self.a;
        a1.is_finite() && a2.is_finite() && a2.abs() < 1.0 && a1.abs() < 1.0 + a2
    }

    /// The same analog prototype at another rate: the bilinear
    /// transform's inverse at `from` Hz, then the transform at `to` Hz.
    /// At `from == to` the section comes back unchanged (to rounding);
    /// elsewhere it differs by the two transforms' frequency warping,
    /// which grows towards either Nyquist.
    pub fn at_rate(&self, from: f64, to: f64) -> Section {
        if from == to {
            return *self;
        }
        // z⁻¹ = (1 − s/k)/(1 + s/k): a polynomial in z⁻¹ times (1 + s/k)²
        // is n0 + n1·s + n2·s².
        let k = 2.0 * from;
        let analog = |c: [f64; 3]| {
            [c[0] + c[1] + c[2], 2.0 * (c[0] - c[2]) / k, (c[0] - c[1] + c[2]) / (k * k)]
        };
        // s = k'(1 − z⁻¹)/(1 + z⁻¹): times (1 + z⁻¹)².
        let kk = 2.0 * to;
        let digital = |n: [f64; 3]| {
            [
                n[0] + n[1] * kk + n[2] * kk * kk,
                2.0 * n[0] - 2.0 * n[2] * kk * kk,
                n[0] - n[1] * kk + n[2] * kk * kk,
            ]
        };
        let b = digital(analog(self.b));
        let a = digital(analog([1.0, self.a[0], self.a[1]]));
        Section {
            b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]],
            a: [a[1] / a[0], a[2] / a[0]],
        }
    }

    /// How long the section rings ([`Tail::from_biquad`] of its poles).
    pub fn tail(&self) -> Tail {
        if self.a == [0.0, 0.0] {
            if self.b[2] != 0.0 {
                Tail::samples(2)
            } else if self.b[1] != 0.0 {
                Tail::samples(1)
            } else {
                Tail::None
            }
        } else {
            Tail::from_biquad(self.a[0], self.a[1])
        }
    }
}

/// A linear block at the rate its model was fitted at.
#[derive(Clone, Debug, PartialEq)]
pub enum Linear {
    /// Cascaded sections (a rational filter); none is the identity.
    Rational(Vec<Section>),
    /// Finite impulse response taps, `y[n] = Σ h[k]·x[n − k]`.
    Fir(Vec<f64>),
}

impl Linear {
    /// The identity: no sections.
    pub fn identity() -> Self {
        Linear::Rational(Vec::new())
    }

    /// A gain alone.
    pub fn gain(g: f64) -> Self {
        Linear::Rational(vec![Section {
            b: [g, 0.0, 0.0],
            a: [0.0, 0.0],
        }])
    }

    /// The response at `hz` for a block at `rate`.
    pub fn response(&self, hz: f64, rate: f64) -> Complex<f64> {
        let w = std::f64::consts::TAU * hz / rate;
        match self {
            Linear::Rational(sections) => sections.iter().fold(Complex::new(1.0, 0.0), |acc, s| acc * s.response(w)),
            Linear::Fir(taps) => taps.iter().enumerate().fold(Complex::new(0.0, 0.0), |acc, (k, &h)| {
                let p = -w * k as f64;
                acc + Complex::new(h * p.cos(), h * p.sin())
            }),
        }
    }

    /// FIR taps: the block runs only at the rate it was fitted at.
    pub fn is_fir(&self) -> bool {
        matches!(self, Linear::Fir(_))
    }

    /// Every coefficient finite, and every section stable.
    pub(crate) fn check(&self) -> Result<(), String> {
        match self {
            Linear::Rational(sections) => {
                for (i, s) in sections.iter().enumerate() {
                    if !s.b.iter().all(|v| v.is_finite()) || !s.is_stable() {
                        return Err(format!("section {i} is unstable or not finite: {s:?}"));
                    }
                }
                Ok(())
            }
            Linear::Fir(taps) if taps.is_empty() => Err("an FIR block with no taps".into()),
            Linear::Fir(taps) if !taps.iter().all(|v| v.is_finite()) => Err("an FIR tap is not finite".into()),
            Linear::Fir(_) => Ok(()),
        }
    }

    /// How long the block rings at its fitted rate.
    pub fn tail(&self) -> Tail {
        match self {
            Linear::Rational(sections) => sections.iter().fold(Tail::None, |t, s| t.then(s.tail())),
            Linear::Fir(taps) => Tail::samples(taps.len().saturating_sub(1) as u32),
        }
    }
}

/// A linear block running: its sections at the prepared rate and their
/// states, or the taps and a doubled history line. Sized when built;
/// `prepare`, `reset` and `tick` never allocate.
#[derive(Clone, Debug)]
pub(crate) struct Running {
    fitted: Vec<Section>,
    sections: Vec<Section>,
    state: Vec<[f64; 2]>,
    taps: Vec<f64>,
    /// The last `taps.len()` inputs twice over, so one contiguous slice
    /// is always the whole history.
    line: Vec<f64>,
    at: usize,
}

impl Running {
    pub(crate) fn new(linear: &Linear) -> Self {
        match linear {
            Linear::Rational(sections) => Self {
                fitted: sections.clone(),
                sections: sections.clone(),
                state: vec![[0.0; 2]; sections.len()],
                taps: Vec::new(),
                line: Vec::new(),
                at: 0,
            },
            Linear::Fir(taps) => Self {
                fitted: Vec::new(),
                sections: Vec::new(),
                state: Vec::new(),
                // Reversed, so the dot product walks both forwards.
                taps: taps.iter().rev().copied().collect(),
                line: vec![0.0; 2 * taps.len()],
                at: 0,
            },
        }
    }

    /// The sections' analog prototypes at `rate` (FIR taps stay).
    pub(crate) fn prepare(&mut self, fitted: f64, rate: f64) {
        for (live, s) in self.sections.iter_mut().zip(&self.fitted) {
            *live = s.at_rate(fitted, rate);
        }
        self.reset();
    }

    pub(crate) fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = [0.0; 2]);
        self.line.iter_mut().for_each(|v| *v = 0.0);
        self.at = 0;
    }

    /// Put the block at rest under a constant input `v` (its steady
    /// state, as a circuit starts at its operating point) and return the
    /// output there.
    pub(crate) fn settle(&mut self, v: f64) -> f64 {
        if self.taps.is_empty() {
            let mut x = v;
            for (s, z) in self.sections.iter().zip(self.state.iter_mut()) {
                // DC gain, then the transposed form's states at rest.
                let y = x * (s.b[0] + s.b[1] + s.b[2]) / (1.0 + s.a[0] + s.a[1]);
                z[0] = y - s.b[0] * x;
                z[1] = s.b[2] * x - s.a[1] * y;
                x = y;
            }
            x
        } else {
            self.line.iter_mut().for_each(|h| *h = v);
            self.at = 0;
            v * self.taps.iter().sum::<f64>()
        }
    }

    /// How long the block rings at the prepared rate.
    pub(crate) fn tail(&self) -> Tail {
        if self.taps.is_empty() {
            self.sections.iter().fold(Tail::None, |t, s| t.then(s.tail()))
        } else {
            Tail::samples(self.taps.len().saturating_sub(1) as u32)
        }
    }

    #[inline]
    pub(crate) fn tick(&mut self, x: f64) -> f64 {
        if self.taps.is_empty() {
            // Transposed direct form II, section by section.
            let mut v = x;
            for (s, z) in self.sections.iter().zip(self.state.iter_mut()) {
                let y = s.b[0] * v + z[0];
                z[0] = s.b[1] * v - s.a[0] * y + z[1];
                z[1] = s.b[2] * v - s.a[1] * y;
                v = y;
            }
            v
        } else {
            let n = self.taps.len();
            self.line[self.at] = x;
            self.line[self.at + n] = x;
            self.at = if self.at + 1 == n { 0 } else { self.at + 1 };
            // line[at..at + n] is the history oldest first; the taps are
            // reversed to match.
            self.line[self.at..self.at + n].iter().zip(&self.taps).map(|(a, b)| a * b).sum()
        }
    }
}
