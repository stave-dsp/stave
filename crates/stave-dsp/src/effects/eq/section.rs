//! A band as a trapezoidal state-variable section whose gain may move
//! every sample: the minimum-phase side of the dynamic EQ.

use super::band::{Band, SHELF_Q};
use crate::kernel::denormal;

/// One band in Simper's linear trapezoidal SVF form (Cytomic, *Linear
/// trap optimised SVF*, 2013): the same bilinear-transformed prototypes
/// as the cookbook biquads (prewarped at the band's frequency), with the
/// gain entering only the mixing and damping coefficients, so it can
/// change per sample without a click or instability (the state is the
/// integrators' charge, which a coefficient change leaves continuous).
#[derive(Copy, Clone, Debug)]
pub(super) struct Section {
    band: Band,
    /// `tan(π·f/fs)`.
    g0: f64,
    /// `1/Q` (bells take `1/(Q·A)`).
    k0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    m0: f64,
    m1: f64,
    m2: f64,
    ic1: f64,
    ic2: f64,
    /// The gain the coefficients are for, dB.
    gain_db: f32,
}

impl Section {
    pub(super) fn new(band: Band, sample_rate: f32) -> Self {
        let fs = f64::from(sample_rate);
        let f = f64::from(band.frequency().get()).clamp(1.0, 0.499 * fs);
        let q = match band {
            Band::Bell { q, .. } | Band::LowCut { q, .. } | Band::HighCut { q, .. } => q,
            Band::LowShelf { .. } | Band::HighShelf { .. } => SHELF_Q,
        };
        let mut s = Self {
            band,
            g0: (core::f64::consts::PI * f / fs).tan(),
            k0: 1.0 / f64::from(q.max(1e-3)),
            a1: 1.0,
            a2: 0.0,
            a3: 0.0,
            m0: 1.0,
            m1: 0.0,
            m2: 0.0,
            ic1: 0.0,
            ic2: 0.0,
            gain_db: f32::NAN,
        };
        s.set_gain_db(band.gain().map_or(0.0, |g| g.get()));
        s
    }

    /// The coefficients for `gain_db` (cuts ignore it). Cheap enough per
    /// sample: one `powf` and one division.
    #[inline]
    pub(super) fn set_gain_db(&mut self, gain_db: f32) {
        if gain_db == self.gain_db {
            return;
        }
        self.gain_db = gain_db;
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        let (g, k, m) = match self.band {
            Band::Bell { .. } => {
                let k = self.k0 / a;
                (self.g0, k, [1.0, k * (a * a - 1.0), 0.0])
            }
            Band::LowShelf { .. } => {
                let k = self.k0;
                (self.g0 / a.sqrt(), k, [1.0, k * (a - 1.0), a * a - 1.0])
            }
            Band::HighShelf { .. } => {
                let k = self.k0;
                (self.g0 * a.sqrt(), k, [a * a, k * (1.0 - a) * a, 1.0 - a * a])
            }
            Band::LowCut { .. } => (self.g0, self.k0, [1.0, -self.k0, -1.0]),
            Band::HighCut { .. } => (self.g0, self.k0, [0.0, 0.0, 1.0]),
        };
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
        [self.m0, self.m1, self.m2] = m;
    }

    /// The integrators' state, to carry across a reshape.
    pub(super) fn state(&self) -> (f64, f64) {
        (self.ic1, self.ic2)
    }

    pub(super) fn restore(&mut self, ic1: f64, ic2: f64) {
        self.ic1 = ic1;
        self.ic2 = ic2;
    }

    pub(super) fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    #[inline]
    pub(super) fn tick(&mut self, x: f32) -> f32 {
        let v0 = f64::from(x);
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = denormal::flush64(2.0 * v1 - self.ic1);
        self.ic2 = denormal::flush64(2.0 * v2 - self.ic2);
        (self.m0 * v0 + self.m1 * v1 + self.m2 * v2) as f32
    }
}
