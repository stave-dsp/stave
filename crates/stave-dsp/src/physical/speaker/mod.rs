//! Loudspeaker drivers: the lumped electro-mechanical model of a
//! moving-coil driver from its Thiele–Small parameters, with Klippel's
//! large-signal nonlinearities.
//!
//! A voice coil in a magnet's gap is a resistor and an inductor in
//! series with a motor: the current pushes the cone with force `Bl·i`,
//! the moving coil generates a back voltage `Bl·v`, and the cone is a
//! mass on a spring with mechanical losses. Thiele (1961) and Small
//! (1972) reduced a driver's low-frequency behaviour to these lumped
//! parameters; Klippel ("Tutorial: Loudspeaker Nonlinearities — Causes,
//! Parameters, Symptoms", JAES 54(10), 2006) made three of them depend
//! on the cone's excursion `x` — the force factor `Bl(x)`, the
//! suspension's stiffness `Kms(x)` and the coil's inductance `Le(x)` —
//! which is where a driver's large-signal distortion and compression
//! come from. The voice coil's inductance is not a pure inductor (eddy
//! currents in the pole piece make it lossy, "semi-inductive"); the
//! LR-2 network [`ThieleSmall::l2`]/[`ThieleSmall::r2`] is the usual
//! second-order approximation of it (a series `Le` and a parallel
//! `L2 ∥ R2`, as Klippel's and Thorborg & Futtrup's models fit it).
//!
//! **What is a claim and what is not.** [`Driver`] solves the lumped
//! model below exactly in the small-signal limit (it is the bilinear
//! transform of the analytic impedance and response, to rounding) and
//! the large-signal equations to the accuracy of an implicit trapezoidal
//! step (held to a 64×-oversampled high-precision solution). It is a
//! *piston*: cone breakup, the dust cap and the surround's resonances
//! above a few hundred hertz are not lumped parameters, so a guitar
//! speaker's characteristic treble is outside this model (the close-mic
//! model in [`cabinet`](super::cabinet) approximates it).
//!
//! ```text
//! u = (Re + Rg)·i + d(Le(x)·i)/dt + u₂ + Bl(x)·v        u₂ = R2·(i − i₂) = L2·di₂/dt
//! Mms·dv/dt = Bl(x)·i − Rms·v − Kms(x)·x + ½·i²·dLe/dx
//! dx/dt = v
//! ```

mod driver;
mod parts;

pub use driver::{Driver, MAX_ITERATIONS};

use super::rotary::SPEED_OF_SOUND;
use crate::primitives::spectral::Complex;
use core::f64::consts::{PI, TAU};

/// The density of air at 20 °C and sea level, kg/m³.
pub const AIR_DENSITY: f64 = 1.204;

/// The reference pressure of dB SPL, Pa.
pub const P_REF: f64 = 20e-6;

/// A driver's small-signal (Thiele–Small) parameters, SI units.
///
/// The electrical side is `Re` with `Le` in series and the LR-2 network
/// `L2 ∥ R2` after it (`l2 = 0`: a pure `Le`); the mechanical side is the
/// moving mass `Mms`, the suspension's compliance `Cms` and its loss
/// `Rms`; `Bl` couples them, `Sd` radiates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ThieleSmall {
    /// Voice-coil DC resistance, Ω.
    pub re: f64,
    /// Voice-coil inductance (the series part of LR-2), H.
    pub le: f64,
    /// LR-2's parallel inductance, H (0: none).
    pub l2: f64,
    /// LR-2's parallel resistance, Ω.
    pub r2: f64,
    /// Force factor, T·m (N/A).
    pub bl: f64,
    /// Moving mass including the air load, kg.
    pub mms: f64,
    /// Suspension compliance, m/N.
    pub cms: f64,
    /// Mechanical resistance, N·s/m (kg/s).
    pub rms: f64,
    /// Effective piston area, m².
    pub sd: f64,
}

impl ThieleSmall {
    /// The parameters from the small-signal set a datasheet prints:
    /// resonance `fs` (Hz), `qms`, `qes`, `vas` (m³), `re` (Ω), `le` (H)
    /// and `sd` (m²), by Small's relations — `Cms = Vas/(ρc²Sd²)`,
    /// `Mms = 1/(ωs²Cms)`, `Rms = ωs·Mms/Qms`, `Bl = √(ωs·Mms·Re/Qes)`.
    pub fn from_small_signal(fs: f64, qms: f64, qes: f64, vas: f64, re: f64, le: f64, sd: f64) -> Self {
        let ws = TAU * fs;
        let cms = vas / (AIR_DENSITY * SPEED_OF_SOUND * SPEED_OF_SOUND * sd * sd);
        let mms = 1.0 / (ws * ws * cms);
        Self {
            re,
            le,
            l2: 0.0,
            r2: 0.0,
            bl: (ws * mms * re / qes).sqrt(),
            mms,
            cms,
            rms: ws * mms / qms,
            sd,
        }
    }

    /// The same driver with an LR-2 network `l2` (H) ∥ `r2` (Ω).
    pub const fn with_lr2(mut self, l2: f64, r2: f64) -> Self {
        self.l2 = l2;
        self.r2 = r2;
        self
    }

    /// Mechanical resonance `fₛ = 1/(2π√(Mms·Cms))`, Hz.
    pub fn resonance(&self) -> f64 {
        1.0 / (TAU * (self.mms * self.cms).sqrt())
    }

    /// Mechanical Q, `ωs·Mms/Rms`.
    pub fn qms(&self) -> f64 {
        TAU * self.resonance() * self.mms / self.rms
    }

    /// Electrical Q, `ωs·Mms·Re/Bl²`.
    pub fn qes(&self) -> f64 {
        TAU * self.resonance() * self.mms * self.re / (self.bl * self.bl)
    }

    /// Total Q, `Qms·Qes/(Qms + Qes)`.
    pub fn qts(&self) -> f64 {
        let (m, e) = (self.qms(), self.qes());
        m * e / (m + e)
    }

    /// Equivalent air volume of the suspension, `ρc²·Cms·Sd²`, m³.
    pub fn vas(&self) -> f64 {
        AIR_DENSITY * SPEED_OF_SOUND * SPEED_OF_SOUND * self.cms * self.sd * self.sd
    }

    /// Reference efficiency (Small 1972): `η₀ = ρ·Bl²·Sd²/(2πc·Mms²·Re)`.
    pub fn efficiency(&self) -> f64 {
        AIR_DENSITY * self.bl * self.bl * self.sd * self.sd
            / (TAU * SPEED_OF_SOUND * self.mms * self.mms * self.re)
    }

    /// Half-space sensitivity, 1 W into `Re` at 1 m, in the
    /// mass-controlled band: `112.1 + 10·log₁₀ η₀` dB SPL.
    pub fn sensitivity(&self) -> f64 {
        // 1 W radiated at η₀ into 2π sr: p² = ρc·η₀·W/(2π r²).
        let p2 = AIR_DENSITY * SPEED_OF_SOUND * self.efficiency() / TAU;
        10.0 * (p2 / (P_REF * P_REF)).log10()
    }

    /// The blocked (motionless) electrical impedance at `hz`:
    /// `Re + jωLe + jωL2·R2/(R2 + jωL2)`.
    pub fn electrical(&self, hz: f64) -> Complex<f64> {
        let w = TAU * hz;
        let mut z = Complex::new(self.re, w * self.le);
        if self.l2 > 0.0 {
            z = z + div(Complex::new(0.0, w * self.l2 * self.r2), Complex::new(self.r2, w * self.l2));
        }
        z
    }

    /// The mechanical impedance `Rms + jωMms + 1/(jωCms)` at `hz`, N·s/m.
    pub fn mechanical(&self, hz: f64) -> Complex<f64> {
        let w = TAU * hz;
        Complex::new(self.rms, w * self.mms - 1.0 / (w * self.cms))
    }

    /// The input impedance at `hz`, Ω: `Ze + Bl²/Zm`.
    pub fn impedance(&self, hz: f64) -> Complex<f64> {
        self.electrical(hz) + div(Complex::new(self.bl * self.bl, 0.0), self.mechanical(hz))
    }

    /// The cone's velocity per volt at `hz` from a source of
    /// `source_ohms`, m/s/V: `Bl / ((Rg + Ze)·Zm + Bl²)`.
    pub fn velocity(&self, hz: f64, source_ohms: f64) -> Complex<f64> {
        let ze = self.electrical(hz) + Complex::new(source_ohms, 0.0);
        div(Complex::new(self.bl, 0.0), ze * self.mechanical(hz) + Complex::new(self.bl * self.bl, 0.0))
    }

    /// The cone's acceleration per volt, `jω·v`, m/s²/V.
    pub fn acceleration(&self, hz: f64, source_ohms: f64) -> Complex<f64> {
        Complex::new(0.0, TAU * hz) * self.velocity(hz, source_ohms)
    }

    /// The cone's excursion per volt, `v/(jω)`, m/V.
    pub fn excursion(&self, hz: f64, source_ohms: f64) -> Complex<f64> {
        div(self.velocity(hz, source_ohms), Complex::new(0.0, TAU * hz))
    }

    /// On-axis far-field SPL at `hz`, `distance` m from the driver in an
    /// infinite baffle (half space), driven by `volts` rms from a
    /// source of `source_ohms`: `p = ρ·Sd·|a|/(2π r)`.
    pub fn spl(&self, hz: f64, volts: f64, distance: f64, source_ohms: f64) -> f64 {
        let a = self.acceleration(hz, source_ohms).norm() * volts;
        let p = AIR_DENSITY * self.sd * a / (TAU * distance);
        20.0 * (p / P_REF).log10()
    }

    /// The piston's radius `√(Sd/π)`, m.
    pub fn radius(&self) -> f64 {
        (self.sd / PI).sqrt()
    }
}

/// `a / b`.
pub(crate) fn div(a: Complex<f64>, b: Complex<f64>) -> Complex<f64> {
    let d = b.norm_sqr();
    Complex::new((a.re * b.re + a.im * b.im) / d, (a.im * b.re - a.re * b.im) / d)
}

/// The most coefficients a [`Polynomial`] holds (order 8, as Klippel's
/// LSI fits go).
pub const MAX_COEFFICIENTS: usize = 9;

/// A polynomial in the excursion `x` (m), `Σ cₖ·xᵏ`: one of
/// [`LargeSignal`]'s relative curves, so `c₀` is normally 1.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Polynomial {
    c: [f64; MAX_COEFFICIENTS],
    n: usize,
}

impl Polynomial {
    /// The constant 1: no variation.
    pub const ONE: Self = Self { c: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], n: 1 };

    /// `Σ c[k]·xᵏ`, lowest order first.
    ///
    /// # Panics
    ///
    /// With no coefficients or more than [`MAX_COEFFICIENTS`].
    pub fn new(c: &[f64]) -> Self {
        assert!(
            !c.is_empty() && c.len() <= MAX_COEFFICIENTS,
            "Polynomial takes 1..={MAX_COEFFICIENTS} coefficients"
        );
        let mut out = Self { c: [0.0; MAX_COEFFICIENTS], n: c.len() };
        out.c[..c.len()].copy_from_slice(c);
        out
    }

    /// Klippel's absolute coefficients (`b₀, b₁ …` in the curve's own
    /// unit per mᵏ) as a curve relative to `b₀`.
    pub fn relative(absolute: &[f64]) -> Self {
        let b0 = absolute.first().copied().unwrap_or(1.0);
        let mut p = Self::new(absolute);
        for c in &mut p.c[..p.n] {
            *c /= b0;
        }
        p
    }

    /// The coefficients, lowest order first.
    pub fn coefficients(&self) -> &[f64] {
        &self.c[..self.n]
    }

    /// Whether this is the constant 1.
    pub fn is_one(&self) -> bool {
        self.c[0] == 1.0 && self.c[1..].iter().all(|&c| c == 0.0)
    }

    /// The value, first and second derivatives at `x` (Horner).
    #[inline]
    pub fn eval(&self, x: f64) -> (f64, f64, f64) {
        let (mut p, mut d1, mut d2) = (0.0, 0.0, 0.0);
        for &c in self.c[..self.n].iter().rev() {
            d2 = d2 * x + 2.0 * d1;
            d1 = d1 * x + p;
            p = p * x + c;
        }
        (p, d1, d2)
    }
}

/// Klippel's large-signal parameters as curves relative to the
/// small-signal values: `Bl(x) = Bl·bl(x)`, `Kms(x) = kms(x)/Cms`,
/// `Le(x) = Le·le(x)`, `x` in metres.
/// The curves are evaluated inside `±range` (where they were fitted)
/// and held at their edge values beyond it; a sample spent beyond is
/// counted ([`Driver::beyond_range`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LargeSignal {
    pub bl: Polynomial,
    pub kms: Polynomial,
    pub le: Polynomial,
    /// The excursion the curves hold to, m.
    pub range: f64,
}

impl LargeSignal {
    /// No nonlinearity: the Thiele–Small model alone.
    pub const LINEAR: Self = Self { bl: Polynomial::ONE, kms: Polynomial::ONE, le: Polynomial::ONE, range: f64::INFINITY };

    /// Symmetric quadratic curves from Klippel's displacement limits
    /// ("Assessment of Voice-Coil Peak Displacement Xmax", JAES 51(5),
    /// 2003): `Bl` falls to 82 % at `x_bl`, the compliance to 75 % at
    /// `x_c` (stiffness to 4/3), the limits that give 10 % distortion.
    /// The curves hold to `range` (m).
    pub fn from_limits(x_bl: f64, x_c: f64, range: f64) -> Self {
        Self {
            bl: Polynomial::new(&[1.0, 0.0, -0.18 / (x_bl * x_bl)]),
            kms: Polynomial::new(&[1.0, 0.0, (1.0 / 0.75 - 1.0) / (x_c * x_c)]),
            le: Polynomial::ONE,
            range,
        }
    }

    /// Whether every curve is the constant 1.
    pub fn is_linear(&self) -> bool {
        self.bl.is_one() && self.kms.is_one() && self.le.is_one()
    }
}

impl Default for LargeSignal {
    fn default() -> Self {
        Self::LINEAR
    }
}

#[cfg(test)]
mod tests;
