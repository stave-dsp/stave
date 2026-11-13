//! A wind instrument's air column — cylindrical or conical, open at the
//! bell — blown through a reed or the lips, as a digital waveguide.
//!
//! The bore is a delay line for the pressure wave's round trip; the
//! open end reflects it inverted, less what it radiates, a little
//! beyond the pipe's end (the end correction); the closed mouthpiece
//! reflects it as the bore's shape says — a cylinder's as is, a
//! truncated cone's through the first-order allpass its spherical waves
//! give. The player's valve sits there, letting through a flow that
//! depends on the pressure across it.

mod valve;

pub use valve::Valve;

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};

/// The speed of sound in dry air at 20 °C, m/s.
pub const SPEED_OF_SOUND: f64 = 343.2;

/// Levine & Schwinger's end correction of an unflanged pipe at low
/// frequency, in radii (Phys. Rev. 73, 1948).
pub const END_CORRECTION: f64 = 0.6133;

/// The bore's shape.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Shape {
    /// A cylinder (clarinet, trumpet's leadpipe-less ideal): resonances
    /// at the odd harmonics, `(2n − 1) c / 4L′`.
    Cylinder,
    /// A cone truncated `apex` metres from its apex at the mouthpiece
    /// (oboe, saxophone, bassoon): resonances where
    /// `tan k L′ = −k·apex`, every harmonic as the truncation shrinks.
    Cone { apex: f64 },
}

/// An air column: its length from mouthpiece to open end, its radius at
/// the open end, its shape.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bore {
    /// Mouthpiece to open end, m.
    pub length: f64,
    /// Radius at the open end, m.
    pub radius: f64,
    pub shape: Shape,
}

impl Bore {
    /// The end correction `δ = 0.6133 a`, m.
    pub fn end_correction(&self) -> f64 {
        END_CORRECTION * self.radius
    }

    /// The acoustic length `L′ = L + δ`, m.
    pub fn acoustic_length(&self) -> f64 {
        self.length + self.end_correction()
    }

    /// The truncation's distance from the apex (0 for a cylinder).
    fn apex(&self) -> f64 {
        match self.shape {
            Shape::Cylinder => 0.0,
            Shape::Cone { apex } => apex.max(0.0),
        }
    }

    /// The `n`-th resonance (from 1) of the closed–open bore with its end
    /// correction, rad/s: `kL′ = (n − ½)π` for a cylinder, the `n`-th
    /// root of `sin kL′ + k x₀ cos kL′ = 0` for a cone.
    pub fn resonance(&self, n: u32) -> f64 {
        let l = self.acoustic_length();
        let n = f64::from(n.max(1));
        let x0 = self.apex();
        let k = match self.shape {
            Shape::Cylinder => (n - 0.5) * core::f64::consts::PI / l,
            Shape::Cone { .. } => {
                let g = |k: f64| (k * l).sin() + k * x0 * (k * l).cos();
                let (mut lo, mut hi) = ((n - 0.5) * core::f64::consts::PI / l, n * core::f64::consts::PI / l);
                let glo = g(lo);
                for _ in 0..80 {
                    let mid = 0.5 * (lo + hi);
                    if g(mid).signum() == glo.signum() {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            }
        };
        k * SPEED_OF_SOUND
    }

    /// Wall losses (visco-thermal) per metre at `hz`: Fletcher &
    /// Rossing's `α ≈ 3×10⁻⁵ √f / a` for a radius `a` in metres, taken
    /// at the bore's mean radius.
    fn wall_loss(&self, hz: f64) -> f64 {
        let mean = match self.shape {
            Shape::Cylinder => self.radius,
            Shape::Cone { apex } => self.radius * (apex.max(0.0) + 0.5 * self.length) / (apex.max(0.0) + self.length),
        };
        3e-5 * hz.max(0.0).sqrt() / mean.max(1e-4)
    }

    /// The open end's reflection magnitude at `hz` with the round trip's
    /// wall losses: `e^{−(ka)²/2}` (Levine & Schwinger at low `ka`)
    /// times `e^{−2Lα}`.
    fn round_trip_loss(&self, hz: f64) -> f64 {
        let ka = core::f64::consts::TAU * hz / SPEED_OF_SOUND * self.radius;
        (-0.5 * ka * ka).exp() * (-2.0 * self.length * self.wall_loss(hz)).exp()
    }
}

/// What the waveguide outputs.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Output {
    /// The pressure in the mouthpiece (what the valve feels).
    Mouthpiece,
    /// The pressure wave the open end lets out (the incident wave less
    /// what it reflects: highpassed), the sound.
    #[default]
    Bell,
}

/// A blown tube: a cylindrical or conical bore as a digital waveguide,
/// excited by a reed or the lips.
///
/// # Models
/// Plane (cylinder) or spherical (cone) pressure waves in a bore closed
/// at the mouthpiece and open at the far end (Smith 1992 and *Physical
/// Audio Signal Processing*, "Conical acoustic tubes"; Benade): the
/// round trip one delay line; the open end's reflection `−R(ω)`, with
/// `|R| = e^{−(ka)²/2}` (Levine & Schwinger at low `ka`) and the walls'
/// visco-thermal loss `3×10⁻⁵ √f/a` per metre (Fletcher & Rossing) fitted
/// by a one-pole lowpass, and its phase the end correction `0.6133 a`;
/// the closed end of a truncated cone, `x₀` from its apex, reflects the
/// spherical wave through `(sτ − 1)/(sτ + 1)`, `τ = x₀/c` (a cylinder:
/// `τ → ∞`, +1). At the mouthpiece a [`Valve`] in Kergomard's
/// dimensionless form (Wilson & Beavers' quasi-static reed; Chaigne &
/// Kergomard ch. 9): pressures over the closing pressure, flow
/// `u = ζ · opening · sgn Δ √|Δ|` across `Δ = γ − p` (`γ` the blowing
/// pressure, `ζ` the embouchure); a reed's opening `(1 − Δ)⁺`, a lip's a
/// resonant mass–spring driven by `Δ`.
///
/// # Derivation
/// Incoming wave `G`, outgoing `F`, `p = F + G`, `u = F − G` at a
/// cylinder's mouthpiece (`Z_c = 1`); a cone's spherical-wave junction
/// `F = A(z) G + H(z) u`, `A = (s τ − 1)/(s τ + 1)`, `H = s τ/(s τ + 1)`
/// (the flow's coupling through the spherical wave's reactive near
/// field), by the bilinear transform prewarped at the first resonance;
/// both share one pole, and a cylinder is its limit `A = H = 1`. So
/// `p = q + h u` with `q`, `h` known before the valve is solved — in
/// `s = sgn Δ √|Δ|`, a monotone cubic (reed: bracketed Newton, capped at
/// 40 iterations, reported) or a quadratic (lips: closed form, the lips'
/// opening from the previous sample). The line is the round trip
/// `2L′/c` less the open end's filter phase delay at the first
/// resonance, its fraction through `I`, compensated
/// (`Interpolator::response`).
///
/// # Verified
/// `crates/stave-dsp/src/physical/tube/tests.rs::cylinder_resonates_at_odd_harmonics`
/// (the passive bore's first six resonances within 10 cents of
/// `(2n − 1)c/4(L + 0.6133a)`, measured 3.9; the even harmonics
/// anti-resonant) and
/// `crates/stave-dsp/src/physical/tube/tests.rs::cone_resonates_where_its_truncation_says`
/// (truncations of 2 and 8 cm: six resonances within 10 cents of the
/// roots of `tan k(L + δ) = −k x₀`, measured 6.1 — the one-pole's phase
/// drifts from the end correction's as `ka` grows),
/// `crates/stave-dsp/src/physical/tube/tests.rs::reed_blows_at_the_first_resonance`
/// (silent at `γ = 0.25`, under Kergomard's lossless threshold of 1/3;
/// sounding at 0.5, at the first resonance within 20 cents, measured
/// 1.6) and
/// `crates/stave-dsp/src/physical/tube/tests.rs::lips_lock_to_the_nearest_resonance`
/// (lips tuned to 0.9 of resonances 2–4 sound at them within 25 cents,
/// measured +11 to +13: outward-striking valves play above their own
/// resonance).
///
/// # Cost
/// `"Tube (clarinet, reed)"`, `"Tube (cone, reed)"` and
/// `"Tube (brass, lips)"` on the datasheet.
///
/// # Aliasing
/// The valve is a nonlinearity: a reed beating at high blowing pressure
/// clips the flow to zero, and its harmonics above Nyquist fold back,
/// tempered by the open end's lowpass (oversample for bright, loud
/// tones).
///
/// # Valid ranges
/// `γ` from 0 to about 2; `ζ` in `[0, 1)` (held below `1/h`, where the
/// mouthpiece's solution stays unique); a bore no longer than the one
/// given at construction; `ka < 1` at the resonances that matter (above,
/// the low-`ka` reflection runs long).
///
/// ```
/// use stave_dsp::physical::tube::{Bore, Shape, Valve, Waveguide};
/// use stave_dsp::Stage;
///
/// let clarinet = Bore { length: 0.6, radius: 0.0075, shape: Shape::Cylinder };
/// let mut tube: Waveguide = Waveguide::new(clarinet, Valve::Reed);
/// tube.set_blow(0.5);
/// tube.set_embouchure(0.3);
/// tube.prepare(48_000.0);
/// let tone: f32 = (0..48_000).map(|_| tube.tick(0.0).abs()).sum();
/// assert!(tone > 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct Waveguide<I: Interpolator = Lagrange3> {
    line: DelayLine<I>,
    bore: Bore,
    longest: f64,
    valve: Valve,
    output: Output,
    blow: f64,
    embouchure: f64,
    sample_rate: f32,
    /// The line's length; the open end's `g(1 − p)/(1 − p z⁻¹)`.
    delay: f32,
    bell: [f32; 2],
    /// The mouthpiece junction's pole `α` and flow coupling `h`.
    alpha: f64,
    coupling: f64,
    /// The lips' resonator: `b0, 2r cos θ, −r²`.
    lips: [f64; 3],
    bell_state: f32,
    junction_state: f64,
    lip_state: [f64; 2],
    delta: f64,
    iterations: u32,
}

impl<I: Interpolator> Waveguide<I> {
    /// A tube of `bore` (and no longer: the line is sized for it) blown
    /// through `valve`: `γ = 0` (silent), `ζ = 0.3`, the bell's output.
    pub fn new(bore: Bore, valve: Valve) -> Self {
        let mut w = Self {
            line: DelayLine::default(),
            bore,
            longest: bore.acoustic_length(),
            valve,
            output: Output::Bell,
            blow: 0.0,
            embouchure: 0.3,
            sample_rate: 48_000.0,
            delay: 1.0,
            bell: [0.0; 2],
            alpha: 1.0,
            coupling: 1.0,
            lips: [0.0; 3],
            bell_state: 0.0,
            junction_state: 0.0,
            lip_state: [0.0; 2],
            delta: 0.0,
            iterations: 0,
        };
        w.update();
        w
    }

    /// The bore (block rate; no longer than the one constructed with).
    pub fn set_bore(&mut self, bore: Bore) {
        self.bore = bore;
        if bore.acoustic_length() > self.longest {
            self.bore.length = self.longest - bore.end_correction();
        }
        self.update();
    }

    /// The bore as set.
    pub fn bore(&self) -> &Bore {
        &self.bore
    }

    /// The valve (block rate).
    pub fn set_valve(&mut self, valve: Valve) {
        self.valve = valve;
        self.update();
    }

    /// The blowing pressure `γ`, over the valve's closing pressure
    /// (realtime).
    pub fn set_blow(&mut self, gamma: f64) {
        self.blow = gamma.max(0.0);
    }

    /// The embouchure `ζ` (realtime): how much flow the open valve lets
    /// through, `Z_c w H √(2/ρ p_M)` in Kergomard's terms.
    pub fn set_embouchure(&mut self, zeta: f64) {
        self.embouchure = zeta.clamp(0.0, 0.99 / self.coupling.max(1e-9));
    }

    /// What [`tick`](Stage::tick) returns.
    pub fn set_output(&mut self, output: Output) {
        self.output = output;
    }

    /// The pressure across the valve last sample, `Δ = γ − p`.
    pub fn pressure_across(&self) -> f64 {
        self.delta
    }

    /// The Newton iterations the reed took last sample (0 for lips or
    /// a shut reed).
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    fn update(&mut self) {
        let fs = f64::from(self.sample_rate);
        let bore = self.bore;
        let w1 = bore.resonance(1) / fs;
        let f1 = w1 * fs / core::f64::consts::TAU;
        // The mouthpiece: a cone's pole by the bilinear transform
        // prewarped at the first resonance; a cylinder's limit.
        let x0 = bore.apex();
        if x0 > 0.0 {
            let k = w1 / (0.5 * w1).tan() * fs;
            let kt = k * x0 / SPEED_OF_SOUND;
            self.alpha = (kt - 1.0) / (kt + 1.0);
            self.coupling = kt / (kt + 1.0);
        } else {
            self.alpha = 1.0;
            self.coupling = 1.0;
        }
        self.embouchure = self.embouchure.min(0.99 / self.coupling);
        // The open end: one-pole magnitude through the first resonance
        // and the fifth (or below 0.45·fs).
        let mut nt = 5.0;
        while nt > 1.5 && bore.resonance(nt as u32) / fs > 0.9 * core::f64::consts::PI {
            nt -= 1.0;
        }
        let wt = bore.resonance(nt as u32) / fs;
        let read = self.line.interpolator();
        let polar = |frac: f64, w: f64| {
            let (re, im) = read.response(frac as f32, w);
            ((re * re + im * im).sqrt(), -im.atan2(re) / w)
        };
        let trip = 2.0 * bore.acoustic_length() / SPEED_OF_SOUND * fs;
        let mut d = trip;
        let (mut pole, mut gain) = (0.0f64, 1.0f64);
        for _ in 0..4 {
            let frac = d.fract();
            let target1 = bore.round_trip_loss(f1) / polar(frac, w1).0;
            let mut p = 0.0;
            if wt > 1.5 * w1 {
                let rho = bore.round_trip_loss(wt * fs / core::f64::consts::TAU) / polar(frac, wt).0 / target1;
                if rho < 1.0 {
                    let r2 = rho * rho;
                    let beta = (w1.cos() - r2 * wt.cos()) / (r2 - 1.0);
                    if beta < -1.0 {
                        p = -beta - (beta * beta - 1.0).sqrt();
                    }
                }
            }
            pole = p;
            gain = (target1 * (1.0 - 2.0 * p * w1.cos() + p * p).sqrt() / (1.0 - p)).min(0.999_99);
            let (s, c) = w1.sin_cos();
            let lag = (p * s).atan2(1.0 - p * c) / w1;
            let (_, tau) = polar(frac, w1);
            d = (d + trip - lag - (d.floor() + tau)).max(I::NEWER as f64 + 1.0);
        }
        self.delay = d as f32;
        self.bell = [(gain * (1.0 - pole)) as f32, pole as f32];
        // The lips: unit DC gain, impulse invariant.
        if let Valve::Lip { frequency, q } = self.valve {
            let w = core::f64::consts::TAU * f64::from(frequency.get()) / fs;
            let q = q.max(0.5);
            let r = (-w / (2.0 * q)).exp();
            let theta = w * (1.0 - 1.0 / (4.0 * q * q)).max(0.0).sqrt();
            let a1 = 2.0 * r * theta.cos();
            let a2 = -r * r;
            self.lips = [1.0 - a1 - a2, a1, a2];
        }
    }
}

impl<I: Interpolator> Stage for Waveguide<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let trip = 2.0 * self.longest / SPEED_OF_SOUND * f64::from(sample_rate);
        self.line.allocate(trip.ceil() as usize + I::OLDER + 4);
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        self.line.reset();
        self.bell_state = 0.0;
        self.junction_state = 0.0;
        self.lip_state = [0.0; 2];
        self.delta = 0.0;
        self.iterations = 0;
    }

    /// `x` is a flow injected at the mouthpiece (beside the valve's).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let incident = self.line.interpolated(self.delay);
        let lp = self.bell[0] * incident + self.bell[1] * self.bell_state;
        self.bell_state = denormal::flush(lp);
        let g = -f64::from(lp);
        // p = q + h·u at the mouthpiece.
        let (alpha, h) = (self.alpha, self.coupling);
        let q = (1.0 + alpha) * g + self.junction_state + h * f64::from(x);
        let (reed, opening) = match self.valve {
            Valve::Reed => (true, 1.0),
            Valve::Lip { .. } => {
                // The lips open with the pressure across them (last sample's).
                let [b0, a1, a2] = self.lips;
                let o = b0 * self.delta + a1 * self.lip_state[0] + a2 * self.lip_state[1];
                self.lip_state = [o, self.lip_state[0]];
                (false, o)
            }
        };
        let (delta, u, iterations) = valve::solve(reed, self.embouchure, opening, self.blow, q, h);
        self.iterations = iterations;
        self.delta = if delta.is_finite() { delta } else { 0.0 };
        let u = if u.is_finite() { u } else { 0.0 } + f64::from(x);
        let f = alpha * g + h * u + self.junction_state;
        let s = -g - h * u + alpha * f;
        self.junction_state = if s.abs() < 1e-30 || !s.is_finite() { 0.0 } else { s };
        let p = f + g;
        self.line.write(denormal::finite_or_zero(f as f32));
        self.line.advance();
        match self.output {
            Output::Mouthpiece => p as f32,
            Output::Bell => incident - lp,
        }
    }

    /// The bore's ring with the valve shut: the open end's loss per
    /// round trip, to −120 dB.
    fn tail(&self) -> Tail {
        let g = f64::from(self.bell[0] / (1.0 - self.bell[1]).max(1e-6)).abs().min(0.999_99);
        if g <= 0.0 {
            return Tail::samples(self.delay.ceil() as u32 + 1);
        }
        Tail::from_pole(g.powf(1.0 / f64::from(self.delay.max(1.0)))).plus(self.delay.ceil() as u32 + 1)
    }
}

#[cfg(test)]
mod tests;
