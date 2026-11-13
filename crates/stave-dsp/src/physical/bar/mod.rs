//! A bar — a marimba or glockenspiel key, a tuning fork's tine, a
//! kalimba's tongue — struck at one point and heard at another, as its
//! bending modes.
//!
//! A thin uniform beam bends by the Euler–Bernoulli equation, whose
//! modes are dispersive: mode `n` rings at `ω_n = (λ_n/L)² √(EI/ρA)`,
//! `λ_n` the roots of `cos λ cosh λ = 1` for a bar free at both ends
//! (1 : 2.756 : 5.404 : 8.933 …) or `cos λ cosh λ = −1` clamped at one
//! (1 : 6.267 : 17.55 …). Real mallet keys are undercut to tune those
//! ratios; this is the uniform bar.

pub use crate::physical::plate::{Loss, Material, Output};

use crate::kernel::node::{Stage, Tail};
use crate::physical::modal::{Body, Ring};
use crate::units::Hz;

/// How the bar is held.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Boundary {
    /// Free at both ends (a xylophone, marimba or glockenspiel key,
    /// resting on cords at its nodes).
    #[default]
    Free,
    /// Clamped at `x = 0`, free at `x = L` (a cantilever: a kalimba
    /// tongue, a music-box tooth, a tuning-fork tine).
    Clamped,
}

/// A uniform bar of rectangular section: length, thickness (in the
/// direction it bends) and material. The width does not enter the
/// frequencies.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Beam {
    /// Length, m.
    pub length: f64,
    /// Thickness, m.
    pub thickness: f64,
    pub material: Material,
}

impl Beam {
    /// `√(EI/ρA) = h √(E/12ρ)`, m²/s, for a rectangular section.
    pub fn stiffness(&self) -> f64 {
        self.thickness * (self.material.young / (12.0 * self.material.density)).sqrt()
    }

    /// The beam of `material` and `thickness` cut to the length whose
    /// first mode, held by `boundary`, rings at `fundamental`.
    pub fn tuned(material: Material, thickness: f64, boundary: Boundary, fundamental: Hz) -> Self {
        let mut beam = Self { length: 1.0, thickness, material };
        let lambda = roots(boundary, 1)[0];
        let w = core::f64::consts::TAU * f64::from(fundamental.get());
        beam.length = lambda * (beam.stiffness() / w).sqrt();
        beam
    }

    /// Mode `n`'s frequency (rad/s) from its root `λ_n`.
    pub fn natural(&self, lambda: f64) -> f64 {
        let k = lambda / self.length;
        k * k * self.stiffness()
    }
}

/// The first `count` roots `λ_n` of the bar's frequency equation:
/// `cos λ cosh λ = 1` (free–free, excluding the rigid-body root at 0)
/// or `−1` (clamped–free), to machine precision. Written as
/// `cos λ = ±sech λ` and bisected inside `((n + ½)π ± ½)`, where each
/// root lies (`sech λ < 0.02` beyond the first two).
pub fn roots(boundary: Boundary, count: usize) -> Vec<f64> {
    let sign = match boundary {
        Boundary::Free => 1.0,
        Boundary::Clamped => -1.0,
    };
    let f = |x: f64| x.cos() - sign / x.cosh();
    (0..count)
        .map(|i| {
            // Free: λ_n ≈ (n + ½)π, n ≥ 1; clamped: λ_n ≈ (n − ½)π.
            let centre = match boundary {
                Boundary::Free => (i as f64 + 1.5) * core::f64::consts::PI,
                Boundary::Clamped => (i as f64 + 0.5) * core::f64::consts::PI,
            };
            let (mut lo, mut hi) = (centre - 0.5, centre + 0.5);
            let flo = f(lo);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if f(mid).signum() == flo.signum() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            0.5 * (lo + hi)
        })
        .collect()
}

/// Mode `λ`'s shape at `ξ = x/L`, normalised so `∫₀¹ φ² dξ = 1`:
/// free–free `cosh + cos − s(sinh + sin)`, clamped–free
/// `cosh − cos − s(sinh − sin)`, written so the hyperbolic terms never
/// cancel catastrophically (`cosh y − s sinh y` as
/// `((1 − s)eʸ + (1 + s)e⁻ʸ)/2`, with `1 − s` in closed form).
pub fn shape(boundary: Boundary, lambda: f64, xi: f64) -> f64 {
    let y = lambda * xi.clamp(0.0, 1.0);
    let (sl, cl) = lambda.sin_cos();
    let em = (-lambda).exp();
    match boundary {
        Boundary::Free => {
            // s = (cosh λ − cos λ)/(sinh λ − sin λ).
            let den = lambda.sinh() - sl;
            let s = (lambda.cosh() - cl) / den;
            let one_minus_s = (-em - sl + cl) / den;
            let hyper = 0.5 * (one_minus_s * y.exp() + (1.0 + s) * (-y).exp());
            hyper + y.cos() - s * y.sin()
        }
        Boundary::Clamped => {
            // s = (cosh λ + cos λ)/(sinh λ + sin λ).
            let den = lambda.sinh() + sl;
            let s = (lambda.cosh() + cl) / den;
            let one_minus_s = (-em + sl - cl) / den;
            let hyper = 0.5 * (one_minus_s * y.exp() + (1.0 + s) * (-y).exp());
            hyper - y.cos() + s * y.sin()
        }
    }
}

/// One mode of a [`Bar`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mode {
    /// The mode's number, from 1.
    pub n: u32,
    /// Its root `λ_n`.
    pub lambda: f64,
    /// Undamped angular frequency `Ω`, rad/s.
    pub natural: f64,
    /// Decay rate `σ`, 1/s.
    pub sigma: f64,
}

impl Mode {
    /// The undamped frequency, Hz.
    pub fn hz(&self) -> f64 {
        self.natural / core::f64::consts::TAU
    }
}

/// A struck bar: the bending modes of a thin uniform beam, free at both
/// ends or clamped at one, driven at one point and heard at another.
///
/// # Models
/// The Euler–Bernoulli beam `ρA ∂²u/∂t² = −EI ∂⁴u/∂x⁴ − 2ρA σ(Ω) ∂u/∂t + δ(x − x_d) f`
/// with free (`u″ = u‴ = 0`) or clamped (`u = u′ = 0`) ends, solved
/// exactly in its modes (Rayleigh; Fletcher & Rossing ch. 2): mode `n`
/// at `Ω = (λ_n/L)² √(EI/ρA)`, with the textbook shapes ([`shape`]).
/// Decay by the plate's two-T60 law ([`Loss`]). No shear or rotary
/// inertia (Timoshenko): thin bars, whose upper modes run a little
/// sharp of a real key's.
///
/// # Derivation
/// `λ_n` by bisection of `cos λ = ±sech λ` ([`roots`]); orthonormal
/// shapes; each mode weighted `φ_n(x_d) φ_n(x_p)/(ρA L)` and run as a
/// Mathews–Smith phasor in the plate's SIMD lanes, the pickup
/// calibrated to a unit-energy impulse response; output as
/// [`Output`] says.
///
/// # Verified
/// `crates/stave-dsp/src/physical/bar/tests.rs::roots_match_beam_theory`
/// (`λ_n` against the published roots 4.730041, 7.853205, … and
/// 1.875104, 4.694091, … to 1e-9, and the asymptote `(n ± ½)π`),
/// `crates/stave-dsp/src/physical/bar/tests.rs::shapes_are_orthonormal`
/// (Simpson's rule, within 1e-6),
/// `crates/stave-dsp/src/physical/bar/tests.rs::mode_ratios_ring_as_beam_theory_says`
/// (the modes at `(λ_n/λ_1)²` of the fundamental to 1e-8, and ringing
/// there in the impulse response within 0.01 cent, both boundaries:
/// measured 9e-5),
/// `crates/stave-dsp/src/physical/bar/tests.rs::a_strike_on_a_node_leaves_its_mode_silent`
/// (mode 2 struck on its node 170 dB under modes 1 and 3) and
/// `crates/stave-dsp/src/physical/bar/tests.rs::block_and_tick_agree_bit_for_bit`.
///
/// # Cost
/// `"Bar (free, 100 Hz)"` on the datasheet (an aluminium bar, every
/// mode below `0.45·fs`, a block at a time).
///
/// # Aliasing
/// None: linear, and every mode lies below `0.45·fs`.
///
/// # Valid ranges
/// Positive dimensions and material constants; drive and pickup in
/// `[0, 1]` of the length; modes up to the bandwidth (at most
/// [`MAX_MODES`]).
///
/// ```
/// use stave_dsp::physical::bar::{Bar, Beam, Boundary, Loss, Material};
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// let key = Beam::tuned(Material::ALUMINIUM, 0.008, Boundary::Free, hz(880.0));
/// let loss = Loss::two_point(hz(880.0), seconds(2.0), hz(8_000.0), seconds(0.4)).unwrap();
/// let mut glock = Bar::new(key, Boundary::Free, loss);
/// glock.prepare(48_000.0);
/// assert!(glock.tick(1.0).is_finite());
/// ```
#[derive(Clone, Debug)]
pub struct Bar {
    beam: Beam,
    boundary: Boundary,
    loss: Loss,
    drive: f64,
    pickup: f64,
    output: Output,
    top: Hz,
    sample_rate: f32,
    modes: Vec<Mode>,
    rings: Vec<Ring>,
    body: Body,
}

/// The most modes a bar keeps.
pub const MAX_MODES: usize = 200;

impl Bar {
    /// A bar of `beam` held by `boundary`, losing energy by `loss`:
    /// struck at 0.37 of its length (off every low mode's node) and
    /// heard at its end; velocity output, modes to 18 kHz. Allocates
    /// here and in `prepare`.
    pub fn new(beam: Beam, boundary: Boundary, loss: Loss) -> Self {
        let mut b = Self {
            beam,
            boundary,
            loss,
            drive: 0.37,
            pickup: 1.0,
            output: Output::Velocity,
            top: Hz::new(18_000.0),
            sample_rate: 48_000.0,
            modes: Vec::new(),
            rings: Vec::new(),
            body: Body::default(),
        };
        b.build();
        b
    }

    /// The beam.
    pub fn beam(&self) -> &Beam {
        &self.beam
    }

    /// The modes kept, by rising frequency.
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// Keep modes up to `top` (takes effect at the next `prepare`).
    pub fn set_bandwidth(&mut self, top: Hz) {
        self.top = top;
    }

    /// The factor from SI units (per newton-sample of force) to the
    /// output.
    pub fn scale(&self) -> f64 {
        self.body.scale()
    }

    /// The loss (block rate, no allocation).
    pub fn set_loss(&mut self, loss: Loss) {
        self.loss = loss;
        for m in &mut self.modes {
            m.sigma = loss.sigma(m.natural);
        }
        self.update();
    }

    /// Move the strike, `0..=1` of the length (block rate).
    pub fn set_drive(&mut self, at: f64) {
        self.drive = at;
        self.update();
    }

    /// Move the pickup, `0..=1` of the length (block rate).
    pub fn set_pickup(&mut self, at: f64) {
        self.pickup = at;
        self.update();
    }

    /// What the pickup reads (block rate).
    pub fn set_output(&mut self, output: Output) {
        self.output = output;
        self.update();
    }

    fn build(&mut self) {
        let top = core::f64::consts::TAU * f64::from(self.top.get().min(0.45 * self.sample_rate));
        self.modes.clear();
        for (i, lambda) in roots(self.boundary, MAX_MODES).into_iter().enumerate() {
            let natural = self.beam.natural(lambda);
            if natural > top {
                break;
            }
            let sigma = self.loss.sigma(natural);
            self.modes.push(Mode { n: i as u32 + 1, lambda, natural, sigma });
        }
        self.rings = vec![Ring { natural: 0.0, sigma: 0.0, amplitude: 0.0 }; self.modes.len()];
        self.body.allocate(self.modes.len());
        self.update();
    }

    fn update(&mut self) {
        let m = &self.beam.material;
        // ρA L with unit width: the modal mass of an orthonormal shape.
        let mass = m.density * self.beam.thickness * self.beam.length;
        for (mode, ring) in self.modes.iter().zip(self.rings.iter_mut()) {
            let shapes = shape(self.boundary, mode.lambda, self.drive) * shape(self.boundary, mode.lambda, self.pickup);
            *ring = Ring { natural: mode.natural, sigma: mode.sigma, amplitude: shapes / mass };
        }
        self.body.set(&self.rings, self.output, self.sample_rate);
    }
}

impl Stage for Bar {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.build();
    }

    fn reset(&mut self) {
        self.body.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.body.tick(x)
    }

    /// Block by block; bit-identical to `tick`.
    fn process(&mut self, block: &mut [f32]) {
        self.body.process(block);
    }

    /// Two of the slowest mode's T60s (−120 dB).
    fn tail(&self) -> Tail {
        self.body.tail(self.sample_rate)
    }
}

#[cfg(test)]
mod tests;
