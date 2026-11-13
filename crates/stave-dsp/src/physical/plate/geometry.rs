//! The plate's physics: its material and dimensions, its losses, and the
//! points where it is driven and heard.

use crate::units::{Hz, Seconds};

/// `3 ln 10`: a T60 is the time `e^(−σt)` takes to fall 60 dB.
pub(super) const LN_1000: f64 = 6.907_755_278_982_137;

/// An isotropic plate material.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Material {
    /// Density, kg/m³.
    pub density: f64,
    /// Young's modulus, Pa.
    pub young: f64,
    /// Poisson's ratio.
    pub poisson: f64,
}

impl Material {
    /// Plain carbon steel, handbook values: 7850 kg/m³, 200 GPa, 0.3.
    pub const STEEL: Self = Self { density: 7_850.0, young: 2.0e11, poisson: 0.3 };
    /// Aluminium, handbook values: 2700 kg/m³, 69 GPa, 0.33.
    pub const ALUMINIUM: Self = Self { density: 2_700.0, young: 6.9e10, poisson: 0.33 };
}

/// A thin rectangular plate: its sides, thickness, in-plane tension and
/// material.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Geometry {
    /// The side `Lx`, m.
    pub width: f64,
    /// The side `Ly`, m.
    pub height: f64,
    /// Thickness `h`, m.
    pub thickness: f64,
    /// In-plane tension per unit length `T0`, N/m.
    pub tension: f64,
    pub material: Material,
}

impl Geometry {
    /// The EMT 140's plate: 2 m × 1 m of 0.5 mm steel (Hanks, R-e/p,
    /// June 1982, and the research notes' unit description). The tension
    /// is not published; 1000 N/m (the top of the DAFx-26 challenge's
    /// range) is an assumption, and it moves only the modes below about
    /// 50 Hz (where `T0·k²` meets `D·k⁴`).
    pub const EMT140: Self = Self {
        width: 2.0,
        height: 1.0,
        thickness: 5e-4,
        tension: 1_000.0,
        material: Material::STEEL,
    };

    /// The DAFx-26 parameter-estimation challenge simulator's default
    /// plate (Ducceschi & Gabrielli 2026, `ModalPlate`): 0.5 m × 1.1 m,
    /// 1 mm, 2430 kg/m³, 67 GPa, ν 0.25, 0.01 N/m.
    pub const DAFX26: Self = Self {
        width: 0.5,
        height: 1.1,
        thickness: 1e-3,
        tension: 0.01,
        material: Material { density: 2_430.0, young: 6.7e10, poisson: 0.25 },
    };

    /// Mass per unit area `ρh`, kg/m².
    pub fn mass(&self) -> f64 {
        self.material.density * self.thickness
    }

    /// Flexural rigidity `D = E h³ / 12(1 − ν²)`, N·m.
    pub fn rigidity(&self) -> f64 {
        let m = self.material;
        m.young * self.thickness.powi(3) / (12.0 * (1.0 - m.poisson * m.poisson))
    }

    /// The squared wavenumber `γ² = (mπ/Lx)² + (nπ/Ly)²` of mode `(m, n)`.
    pub fn wavenumber_squared(&self, m: u32, n: u32) -> f64 {
        let kx = f64::from(m) * core::f64::consts::PI / self.width;
        let ky = f64::from(n) * core::f64::consts::PI / self.height;
        kx * kx + ky * ky
    }

    /// Mode `(m, n)`'s undamped angular frequency, rad/s:
    /// `Ω² = (T0 γ² + D γ⁴)/ρh` (Ducceschi & Gabrielli eq. 8).
    pub fn natural(&self, m: u32, n: u32) -> f64 {
        let g2 = self.wavenumber_squared(m, n);
        ((self.tension * g2 + self.rigidity() * g2 * g2) / self.mass()).sqrt()
    }

    /// The largest mode index along a side of length `side` whose mode
    /// `(index, 1)` could still lie at or below `omega` — the bound the
    /// enumeration walks to.
    pub(super) fn index_bound(&self, side: f64, omega: f64) -> u32 {
        let (t, d, mu) = (self.tension, self.rigidity(), self.mass());
        // Solve D γ⁴ + T0 γ² − ρh Ω² = 0 for γ², then γ = mπ/side.
        let g2 = (-t + (t * t + 4.0 * omega * omega * mu * d).sqrt()) / (2.0 * d);
        (side / core::f64::consts::PI * g2.max(0.0).sqrt()).floor() as u32
    }

    /// The modal shape `Φ(x, y) = 2/√(Lx Ly) · sin(mπx/Lx) sin(nπy/Ly)`
    /// at `point` (fractions of the sides), orthonormal over the plate.
    pub fn shape(&self, m: u32, n: u32, point: Point) -> f64 {
        let pi = core::f64::consts::PI;
        2.0 / (self.width * self.height).sqrt()
            * (f64::from(m) * pi * point.x).sin()
            * (f64::from(n) * pi * point.y).sin()
    }

    /// The asymptotic count of modes up to `omega` (rad/s) of a
    /// simply supported plate without tension: `S γ²/4π − P γ/4π`,
    /// `γ² = Ω √(ρh/D)` (Weyl's law with its perimeter term).
    pub fn mode_count(&self, omega: f64) -> f64 {
        let g2 = omega * (self.mass() / self.rigidity()).sqrt();
        let area = self.width * self.height;
        let perimeter = 2.0 * (self.width + self.height);
        (area * g2 - perimeter * g2.sqrt()) / (4.0 * core::f64::consts::PI)
    }
}

/// A point on the plate as fractions of its sides, each in `(0, 1)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Frequency-dependent loss: every mode decays as `e^(−σt)` with
/// `σ(Ω) = η0 + η1 Ω²` (Ducceschi & Gabrielli eq. 10–11; the damped
/// Kirchhoff–Love plate of Bilbao 2007). Set by two decay times at two
/// frequencies.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Loss {
    eta0: f64,
    eta1: f64,
}

/// [`Loss::emt140`]'s high-frequency loss `η1`, s: a T60 of 1 s at
/// 5 kHz with the damper open (5 s at 500 Hz). A voicing choice: the
/// EMT 140's decay above 500 Hz is not published in the sources at hand.
const EMT140_ETA1: f64 = (LN_1000 / 1.0 - LN_1000 / 5.0)
    / (4.0 * core::f64::consts::PI * core::f64::consts::PI * (5_000.0 * 5_000.0 - 500.0 * 500.0));

impl Loss {
    /// The DAFx-26 challenge's fixed loss: T60 6 s at DC and 2 s at
    /// 500 Hz.
    pub const DAFX26: Self = Self::ducceschi(6.0, 2.0, 500.0);

    /// The challenge code's form (eq. 11 with `Ω0 = 0`): T60 `t0` at DC
    /// and `t1` at `f1` Hz.
    const fn ducceschi(t0: f64, t1: f64, f1: f64) -> Self {
        let w1 = 2.0 * core::f64::consts::PI * f1;
        Self { eta0: LN_1000 / t0, eta1: LN_1000 / (w1 * w1) * (t0 - t1) / (t0 * t1) }
    }

    /// The loss through two points: T60 `t60_a` at `at_a` and `t60_b` at
    /// `at_b` (`at_a < at_b`; `at_a` may be 0 Hz). `None` unless the
    /// result is passive at every frequency (`η0, η1 ≥ 0`: the decay
    /// never lengthens with frequency, nor goes negative below `at_a`).
    pub fn two_point(at_a: Hz, t60_a: Seconds, at_b: Hz, t60_b: Seconds) -> Option<Self> {
        let w = |f: Hz| 2.0 * core::f64::consts::PI * f64::from(f.get());
        let (wa, wb) = (w(at_a), w(at_b));
        let (sa, sb) = (LN_1000 / t60_a.get(), LN_1000 / t60_b.get());
        if wb.is_nan() || wb <= wa {
            return None;
        }
        let eta1 = (sb - sa) / (wb * wb - wa * wa);
        let eta0 = sa - eta1 * wa * wa;
        Self::from_coefficients(eta0, eta1)
    }

    /// The loss with these coefficients: `η0` (1/s) and `η1` (s).
    /// `None` unless both are finite and non-negative.
    pub fn from_coefficients(eta0: f64, eta1: f64) -> Option<Self> {
        (eta0.is_finite() && eta1.is_finite() && eta0 >= 0.0 && eta1 >= 0.0).then_some(Self { eta0, eta1 })
    }

    /// An EMT 140 with its damper set for `t60_at_500` at 500 Hz. The
    /// damper — a porous panel swung toward the plate — adds a loss that
    /// does not depend on frequency (`η0`); the plate's own loss above it
    /// (`η1`) stays: T60 at 5 kHz is 1 s with the damper open (a
    /// voicing choice, see the module docs). The unit's published range is
    /// about 0.8 to 5 s at 500 Hz; `None` past about 120 s, where the
    /// damper would have to add energy.
    pub fn emt140(t60_at_500: Seconds) -> Option<Self> {
        let w = 2.0 * core::f64::consts::PI * 500.0;
        Self::from_coefficients(LN_1000 / t60_at_500.get() - EMT140_ETA1 * w * w, EMT140_ETA1)
    }

    /// `(η0, η1)`.
    pub fn coefficients(&self) -> (f64, f64) {
        (self.eta0, self.eta1)
    }

    /// The decay rate `σ` (1/s) of a mode at `omega` rad/s.
    pub fn sigma(&self, omega: f64) -> f64 {
        self.eta0 + self.eta1 * omega * omega
    }

    /// The T60 at `hz`.
    pub fn t60(&self, hz: f64) -> Seconds {
        Seconds::new(LN_1000 / self.sigma(2.0 * core::f64::consts::PI * hz))
    }
}

/// What a pickup reads at its point.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Output {
    /// Transverse displacement (the DAFx-26 simulator's output): `1/Ω`
    /// per mode, so the response falls 6 dB an octave.
    Displacement,
    /// Velocity: a moving-coil or electrodynamic pickup; flat on
    /// average, the default.
    #[default]
    Velocity,
    /// Acceleration: a piezoelectric accelerometer (the EMT 140's
    /// contact pickups, per Hanks 1982); rising 6 dB an octave.
    Acceleration,
}
