//! A linear lowpass as Holters & Parker run it: the partial fractions
//! `H(s) = Σ rₘ / (s − pₘ)` of its circuit, so its response can be
//! evaluated at any instant between two host samples.

use crate::circuit::methods::dk::Phasor;
use crate::units::{Farads, Ohms};

/// The most poles a [`Filter`] holds (conjugates counted apart).
pub const MAX_POLES: usize = 8;

/// A complex number in `f64`, for poles, residues and filter states.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub(super) struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    #[inline]
    pub fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
    #[inline]
    pub fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
    #[inline]
    pub fn mul(self, o: C64) -> C64 {
        C64::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    #[inline]
    pub fn scale(self, k: f64) -> C64 {
        C64::new(self.re * k, self.im * k)
    }
    pub fn div(self, o: C64) -> C64 {
        let d = o.re * o.re + o.im * o.im;
        C64::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
    /// `e^self`.
    #[inline]
    pub fn exp(self) -> C64 {
        let m = self.re.exp();
        let (s, c) = self.im.sin_cos();
        C64::new(m * c, m * s)
    }
    pub fn sqrt(self) -> C64 {
        let r = (self.re * self.re + self.im * self.im).sqrt();
        let re = (0.5 * (r + self.re)).max(0.0).sqrt();
        let im = (0.5 * (r - self.re)).max(0.0).sqrt();
        C64::new(re, if self.im < 0.0 { -im } else { im })
    }
}

/// One unity-gain Sallen–Key lowpass section, as a schematic gives it:
/// the input through `r1` then `r2` to the follower's input, `c1` from
/// the node between the resistors back to the output, `c2` from the
/// follower's input to ground. Its response is
/// `1 / (s²·r1·r2·c1·c2 + s·c2·(r1 + r2) + 1)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SallenKey {
    pub r1: Ohms,
    pub r2: Ohms,
    pub c1: Farads,
    pub c2: Farads,
}

/// A continuous-time lowpass in partial fractions, `H(s) = Σ rₘ/(s − pₘ)`:
/// simple poles in the left half plane, complex ones in conjugate
/// pairs, no direct term (a strictly proper response).
///
/// Built from a circuit's sections ([`sallen_key`](Self::sallen_key)) or
/// from published coefficients ([`JUNO_60_INPUT`](Self::JUNO_60_INPUT));
/// [`HoltersParker`](super::HoltersParker) runs it between the host rate
/// and the BBD clock, and [`response`](Self::response) is the analog
/// response the circuit is checked against.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Filter {
    pub(super) poles: [C64; MAX_POLES],
    pub(super) residues: [C64; MAX_POLES],
    pub(super) len: usize,
}

/// Build a filter from `(re, im)` pairs in a `const` context.
const fn table<const N: usize>(poles: [(f64, f64); N], residues: [(f64, f64); N]) -> Filter {
    let mut f = Filter {
        poles: [C64::new(0.0, 0.0); MAX_POLES],
        residues: [C64::new(0.0, 0.0); MAX_POLES],
        len: N,
    };
    let mut i = 0;
    while i < N {
        f.poles[i] = C64::new(poles[i].0, poles[i].1);
        f.residues[i] = C64::new(residues[i].0, residues[i].1);
        i += 1;
    }
    f
}

impl Filter {
    /// The fifth-order lowpass in front of the Juno-60 chorus's BBD
    /// (Holters & Parker, DAFx-18, Table 1, `H_in`; its first-order
    /// bias highpass left out, as there): poles and residues in rad/s.
    pub const JUNO_60_INPUT: Filter = table(
        [
            (-46_580.0, 0.0),
            (-55_482.0, 25_082.0),
            (-55_482.0, -25_082.0),
            (-26_292.0, -59_437.0),
            (-26_292.0, 59_437.0),
        ],
        [
            (251_589.0, 0.0),
            (-130_428.0, -4_165.0),
            (-130_428.0, 4_165.0),
            (4_634.0, -22_873.0),
            (4_634.0, 22_873.0),
        ],
    );

    /// The fifth-order lowpass after the Juno-60 chorus's BBD
    /// (Holters & Parker, Table 1, `H_out`).
    pub const JUNO_60_OUTPUT: Filter = table(
        [
            (-176_261.0, 0.0),
            (-51_468.0, 21_437.0),
            (-51_468.0, -21_437.0),
            (-26_276.0, -59_699.0),
            (-26_276.0, 59_699.0),
        ],
        [
            (5_092.0, 0.0),
            (11_256.0, -99_566.0),
            (11_256.0, 99_566.0),
            (-13_802.0, -24_606.0),
            (-13_802.0, 24_606.0),
        ],
    );

    /// A cascade of unity-gain Sallen–Key sections (buffered, so the
    /// response is the product of theirs), in partial fractions: the
    /// poles of each section's quadratic, the residues of the all-pole
    /// product. `None` for no sections, more than [`MAX_POLES`] / 2
    /// sections, or a non-positive part value.
    pub fn sallen_key(sections: &[SallenKey]) -> Option<Filter> {
        if sections.is_empty() || 2 * sections.len() > MAX_POLES {
            return None;
        }
        let mut f = Filter {
            poles: [C64::default(); MAX_POLES],
            residues: [C64::default(); MAX_POLES],
            len: 2 * sections.len(),
        };
        // H(s) = 1 / (lead · Π (s − pⱼ)), lead the product of the s² terms.
        let mut lead = 1.0;
        for (i, s) in sections.iter().enumerate() {
            let (r1, r2, c1, c2) = (s.r1.get(), s.r2.get(), s.c1.get(), s.c2.get());
            if !(r1 > 0.0 && r2 > 0.0 && c1 > 0.0 && c2 > 0.0) {
                return None;
            }
            let a = r1 * r2 * c1 * c2;
            let b = c2 * (r1 + r2);
            lead *= a;
            let disc = C64::new(b * b - 4.0 * a, 0.0).sqrt();
            f.poles[2 * i] = C64::new(-b, 0.0).add(disc).scale(0.5 / a);
            f.poles[2 * i + 1] = C64::new(-b, 0.0).sub(disc).scale(0.5 / a);
        }
        for m in 0..f.len {
            let mut d = C64::new(lead, 0.0);
            for j in 0..f.len {
                if j != m {
                    d = d.mul(f.poles[m].sub(f.poles[j]));
                }
            }
            f.residues[m] = C64::new(1.0, 0.0).div(d);
        }
        Some(f)
    }

    /// The analog response `H(j·2π·hz)`.
    pub fn response(&self, hz: f64) -> Phasor {
        let s = C64::new(0.0, core::f64::consts::TAU * hz);
        let mut h = C64::default();
        for m in 0..self.len {
            h = h.add(self.residues[m].div(s.sub(self.poles[m])));
        }
        Phasor::new(h.re, h.im)
    }

    /// The DC gain, `−Σ rₘ/pₘ`.
    pub fn dc_gain(&self) -> f64 {
        let mut g = 0.0;
        for m in 0..self.len {
            g -= self.residues[m].div(self.poles[m]).re;
        }
        g
    }

    /// The poles in rad/s, as `(re, im)`.
    pub fn poles(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.poles[..self.len].iter().map(|p| (p.re, p.im))
    }

    /// The slowest pole's decay rate (the smallest `|Re p|`), in 1/s.
    pub(super) fn slowest_decay(&self) -> f64 {
        self.poles[..self.len].iter().map(|p| -p.re).fold(f64::INFINITY, f64::min)
    }
}
