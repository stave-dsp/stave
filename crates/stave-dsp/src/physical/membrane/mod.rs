//! A circular membrane — a drumhead — struck at one point and heard at
//! another, as its modes.
//!
//! An ideal membrane under uniform tension, clamped at its rim, rings
//! in modes `(m, n)` at `f_mn = j_mn c / 2πa`: `j_mn` the `n`-th zero of
//! the Bessel function `J_m`, `c = √(T/σ)` the wave speed, `a` the
//! radius. The modes are inharmonic (1 : 1.594 : 2.136 : 2.296 …), which
//! is why a timpani's air loading and a tom's two heads are what make
//! drums pitched; neither is modelled here.

mod bessel;

pub use crate::physical::plate::{Loss, Output};

use crate::kernel::node::{Stage, Tail};
use crate::physical::modal::{Body, Ring};
use crate::units::Hz;

/// A drumhead: radius, tension and surface density.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Drumhead {
    /// Radius, m.
    pub radius: f64,
    /// Tension per unit length of edge, N/m.
    pub tension: f64,
    /// Mass per unit area, kg/m².
    pub density: f64,
}

/// `j_01`, the first zero of `J_0` (Abramowitz & Stegun table 9.5).
const J01: f64 = 2.404_825_557_695_773;

impl Drumhead {
    /// A head of `radius` and `density` tensioned so its fundamental
    /// `(0, 1)` mode rings at `fundamental`.
    pub fn tuned(radius: f64, density: f64, fundamental: Hz) -> Self {
        let c = core::f64::consts::TAU * radius * f64::from(fundamental.get()) / J01;
        Self { radius, tension: c * c * density, density }
    }

    /// The transverse wave speed `√(T/σ)`, m/s.
    pub fn wave_speed(&self) -> f64 {
        (self.tension / self.density).sqrt()
    }

    /// Mode `(m, n)`'s frequency, rad/s, from its Bessel zero `j_mn`.
    pub fn natural(&self, zero: f64) -> f64 {
        zero * self.wave_speed() / self.radius
    }
}

/// A point on the head: `r` from the centre as a fraction of the
/// radius, at `angle` radians.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Polar {
    pub r: f64,
    pub angle: f64,
}

impl Polar {
    pub const fn new(r: f64, angle: f64) -> Self {
        Self { r, angle }
    }
}

/// One mode of a [`Membrane`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mode {
    /// Nodal diameters.
    pub m: u32,
    /// Nodal circles, counting the rim.
    pub n: u32,
    /// The Bessel zero `j_mn`.
    pub zero: f64,
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

/// A struck circular membrane: the modes of an ideal drumhead clamped
/// at its rim, driven at one point and heard at another.
///
/// # Models
/// The wave equation on a disc, `σ ∂²u/∂t² = T Δu − 2σ ς(Ω) ∂u/∂t + δ(x − x_d) f`,
/// `u = 0` at `r = a`, solved exactly in its modes: `(m, n)` rings at
/// `Ω = j_mn c/a` and has the shape `J_m(j_mn r/a) cos mθ` (and its
/// `sin mθ` twin when `m > 0`), Morse & Ingard's and Rossing &
/// Fletcher's textbook drumhead. The decay is the plate's two-T60 law
/// `σ = η0 + η1 Ω²` ([`Loss`]). No air loading, no second head, no
/// tension modulation: the ideal membrane.
///
/// # Derivation
/// The zeros `j_mn` are found to machine precision (Miller's
/// recurrence for `J_m`, Newton on bracketed sign changes:
/// `bessel.rs`). With orthonormal shapes `Φ_mn`
/// (`∫ J_m(j r/a)² r dr = a² J_{m+1}(j)²/2`, `∫ cos² mθ dθ = 2π` or `π`),
/// a degenerate pair driven at `(r_d, θ_d)` and heard at `(r_p, θ_p)`
/// sums to one mode weighted `J_m(j r_d) J_m(j r_p) cos m(θ_d − θ_p)`
/// over the modal mass; each mode's sampled impulse response is a
/// Mathews–Smith phasor, eight to a SIMD group (the plate's lanes), the
/// pickup calibrated to a unit-energy impulse response. Displacement,
/// velocity or acceleration as for the plate ([`Output`]).
///
/// # Verified
/// `crates/stave-dsp/src/physical/membrane/tests.rs::bessel_zeros_match_the_tables`
/// (`j_mn`, m = 0…3, n = 1…4, against Abramowitz & Stegun's table 9.5
/// within 1e-12, measured 9e-15; none missing below 17),
/// `crates/stave-dsp/src/physical/membrane/tests.rs::mode_ratios_ring_as_bessel_zeros_say`
/// (the modes in the table's order and ratios to 1e-12; the ten lowest
/// ring in the impulse response within 0.01 cent of `j_mn c/a`,
/// measured 1.2e-4),
/// `crates/stave-dsp/src/physical/membrane/tests.rs::a_centre_strike_rings_only_axisymmetric_modes`
/// (`m > 0` modes 70 dB under the weakest `(0, n)`, at the measurement's
/// floor),
/// `crates/stave-dsp/src/physical/membrane/tests.rs::decay_follows_the_loss_law`
/// (eight modes' T60 within 0.5 % of the law, measured 0.055 %) and
/// `crates/stave-dsp/src/physical/membrane/tests.rs::block_and_tick_agree_bit_for_bit`.
///
/// # Cost
/// `"Membrane (to 5 kHz)"` on the datasheet (a 0.18 m head at 100 Hz, a
/// block at a time, 1799 modes): like the plate, linear in the mode
/// count, which grows as the square of the bandwidth.
///
/// # Aliasing
/// None: linear, and every mode lies below `0.45·fs`.
///
/// # Valid ranges
/// Positive radius, tension and density; drive and pickup with
/// `0 ≤ r < 1` (the rim is a node of every mode); bandwidth up to
/// `0.45·fs`. The mode count grows as the square of the bandwidth over
/// the fundamental: ~`(j_max/2)²/2`.
///
/// ```
/// use stave_dsp::physical::membrane::{Drumhead, Loss, Membrane};
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// let head = Drumhead::tuned(0.18, 0.26, hz(110.0));
/// let loss = Loss::two_point(hz(110.0), seconds(1.2), hz(2_000.0), seconds(0.3)).unwrap();
/// let mut tom = Membrane::new(head, loss);
/// tom.prepare(48_000.0);
/// let hit = tom.tick(1.0);
/// assert!(hit.is_finite());
/// ```
#[derive(Clone, Debug)]
pub struct Membrane {
    head: Drumhead,
    loss: Loss,
    drive: Polar,
    pickup: Polar,
    output: Output,
    top: Hz,
    sample_rate: f32,
    modes: Vec<Mode>,
    rings: Vec<Ring>,
    /// `J_{m+1}(j_mn)²` per mode (the norm), and a Bessel scratch row.
    norms: Vec<f64>,
    scratch: Vec<f64>,
    body: Body,
}

impl Membrane {
    /// A membrane of `head` losing energy by `loss`, struck a third of
    /// the way out and heard at 0.6 of the radius, 2 rad round; velocity
    /// output, modes to 5 kHz. Allocates here and in `prepare`.
    pub fn new(head: Drumhead, loss: Loss) -> Self {
        let mut m = Self {
            head,
            loss,
            drive: Polar::new(0.33, 0.0),
            pickup: Polar::new(0.6, 2.0),
            output: Output::Velocity,
            top: Hz::new(5_000.0),
            sample_rate: 48_000.0,
            modes: Vec::new(),
            rings: Vec::new(),
            norms: Vec::new(),
            scratch: Vec::new(),
            body: Body::default(),
        };
        m.build();
        m
    }

    /// The head.
    pub fn head(&self) -> &Drumhead {
        &self.head
    }

    /// The modes kept, by rising frequency.
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// The highest mode frequency kept.
    pub fn bandwidth(&self) -> Hz {
        Hz::new(self.top.get().min(0.45 * self.sample_rate))
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

    /// Move the strike (block rate, no allocation).
    pub fn set_drive(&mut self, at: Polar) {
        self.drive = at;
        self.update();
    }

    /// Move the pickup (block rate, no allocation).
    pub fn set_pickup(&mut self, at: Polar) {
        self.pickup = at;
        self.update();
    }

    /// What the pickup reads (block rate, no allocation).
    pub fn set_output(&mut self, output: Output) {
        self.output = output;
        self.update();
    }

    /// Enumerate the modes below the bandwidth (allocates).
    fn build(&mut self) {
        let top = core::f64::consts::TAU * f64::from(self.bandwidth().get());
        let limit = top * self.head.radius / self.head.wave_speed();
        let mut zeros = Vec::new();
        bessel::zeros(limit.ceil() as usize + 2, limit, &mut zeros);
        self.modes.clear();
        self.norms.clear();
        let orders = zeros.iter().map(|z| z.0 as usize).max().unwrap_or(0) + 2;
        self.scratch = vec![0.0; orders + 1];
        for &(m, n, zero) in &zeros {
            let natural = self.head.natural(zero);
            let sigma = self.loss.sigma(natural);
            self.modes.push(Mode { m, n, zero, natural, sigma });
            bessel::bessel_j(zero, &mut self.scratch[..m as usize + 2]);
            let next = self.scratch[m as usize + 1];
            self.norms.push(next * next);
        }
        self.rings = vec![Ring { natural: 0.0, sigma: 0.0, amplitude: 0.0 }; self.modes.len()];
        self.body.allocate(self.modes.len());
        self.update();
    }

    /// The modal weights at the drive and pickup, and the body's
    /// coefficients (no allocation).
    fn update(&mut self) {
        let a = self.head.radius;
        let mass = self.head.density * a * a / 2.0;
        let (d, p) = (self.drive, self.pickup);
        let scratch = &mut self.scratch;
        for ((mode, ring), norm) in self.modes.iter().zip(self.rings.iter_mut()).zip(&self.norms) {
            let m = mode.m as usize;
            let row = &mut scratch[..m + 1];
            bessel::bessel_j(mode.zero * d.r.clamp(0.0, 1.0), row);
            let jd = row[m];
            bessel::bessel_j(mode.zero * p.r.clamp(0.0, 1.0), row);
            let jp = row[m];
            let angular = if m == 0 { core::f64::consts::TAU } else { core::f64::consts::PI };
            let shapes = jd * jp * (mode.m as f64 * (d.angle - p.angle)).cos();
            let amplitude = shapes / (mass * norm * angular);
            *ring = Ring { natural: mode.natural, sigma: mode.sigma, amplitude };
        }
        self.body.set(&self.rings, self.output, self.sample_rate);
    }
}

impl Stage for Membrane {
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
