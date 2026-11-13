//! The driver as a stage: the lumped equations stepped by the implicit
//! trapezoidal rule.

use super::{LargeSignal, ThieleSmall, AIR_DENSITY};
use crate::kernel::node::{Stage, Tail};
use core::f64::consts::TAU;

/// Newton iterations a large-signal step may take.
pub const MAX_ITERATIONS: u32 = 16;

/// The states: coil current, LR-2 branch current, cone velocity, excursion.
type State = [f64; 4];
type Matrix = [[f64; 4]; 4];

/// A moving-coil loudspeaker driver, voltage driven: the Thiele–Small
/// model with an LR-2 coil and Klippel's large-signal `Bl(x)`, `Kms(x)`,
/// `Le(x)`.
///
/// The input is a voltage (one unit is [`set_volts`](Self::set_volts)
/// volts) from an amplifier of output resistance
/// [`set_source_ohms`](Self::set_source_ohms); the output is the on-axis
/// far-field pressure, which is the cone's acceleration, normalised so a
/// small signal in the mass-controlled band comes out at unity (so the
/// drive level buys excursion and distortion, not loudness). The coil
/// current, cone velocity, excursion and the pressure in pascals are
/// read after each sample.
///
/// # Models
/// A piston driver's lumped electro-mechanical equations (Thiele 1961,
/// Small 1972), the voice coil's inductance as Le plus an LR-2 network,
/// and the large-signal model of Klippel (JAES 2006, eqs. 1–2):
/// excursion-dependent force factor, stiffness and inductance, with the
/// reluctance force `½·i²·dLe/dx` and the back EMF of the moving
/// inductance `i·dLe/dx·v`. Not modelled: cone breakup and the
/// radiation's directivity (a piston), voice-coil heating, the
/// suspension's creep and its velocity-dependent loss, the current
/// dependence of `Le` and `Bl`, and LR-2's own excursion dependence.
///
/// # Derivation
/// The equations in the [module documentation](super), states `(i, i₂,
/// v, x)`, stepped by the implicit trapezoidal rule on the conserved
/// quantities — the coil's flux `Le(x)·i` (whose rate is the voltage
/// balance, so the moving inductance's back EMF `i·dLe/dx·v` is in it
/// by construction), the LR-2 branch's flux, the cone's momentum and
/// its position. The rule is A-stable, so the coil's electrical pole far
/// above the audio band and a stiffening spring cannot destabilise it;
/// with `Le = 0` the coil's balance is algebraic and is held at each
/// step's end. With [`LargeSignal::LINEAR`] the step is a fixed 4×4
/// update computed at `prepare`; otherwise Newton's method solves each
/// step (analytic Jacobian, at most [`MAX_ITERATIONS`], a cap hit
/// counted). On-axis pressure in a baffle
/// (half space) is `ρ·Sd·a/(2πr)`.
///
/// # Verified
/// `crates/stave-dsp/src/physical/speaker/tests.rs`: the measured
/// impedance and acceleration are the analytic small-signal ones at the
/// bilinear transform's frequency to 1e-9
/// (`small_signal_is_the_bilinear_transform_of_the_analytic_model`),
/// within 0.025 dB of the analytic ones below 2 kHz at 48 kHz; the
/// impedance peaks at `fs` at `Re·(1 + Qms/Qes)`
/// (`the_impedance_peaks_at_fs`); the response is a second-order
/// highpass of Q `Qts`, falling 12 dB an octave below it
/// (`the_response_is_a_qts_highpass`); the large-signal solution's
/// harmonics against a 64×-oversampled Runge–Kutta solution of the same
/// equations (`large_signal_matches_an_oversampled_reference`) and
/// Klippel's symptoms (`klippel_symptoms`). Against real guitar drivers'
/// datasheets: `crates/stave-test/tests/speaker_matches_datasheet.rs`.
///
/// # Cost
/// `"Driver (Thiele–Small)"` and `"Driver (large signal)"` on the
/// datasheet.
///
/// # Aliasing
/// None for a small signal (linear). The large-signal curves are smooth
/// polynomials acting on the excursion, which is lowpassed 12 dB an
/// octave above `fs`, so its harmonics are of the bass; harmonics of the
/// current through `Bl(x)` reach the treble only as far as the drive
/// does. No oversampling is applied.
///
/// # Valid ranges
/// `Re, Bl, Mms, Cms, Rms, Sd > 0`, `Le, L2, R2 ≥ 0`; sample rates from 8
/// kHz. The large-signal curves are trusted inside their `range` and
/// held beyond it ([`beyond_range`](Self::beyond_range) counts samples
/// spent there).
#[derive(Clone, Debug)]
pub struct Driver {
    ts: ThieleSmall,
    large: LargeSignal,
    source_ohms: f64,
    volts: f64,
    /// Half a sample period.
    h: f64,
    /// The linear step: `s₁ = m·s₀ + n₀·u₀ + n₁·u₁`.
    m: Matrix,
    n0: State,
    n1: State,
    s: State,
    u: f64,
    accel: f64,
    /// An LR-2 branch is present.
    lr2: bool,
    /// `Le = 0`: the coil's equation is algebraic, solved at each
    /// sample's end rather than integrated.
    algebraic: bool,
    iterations: u32,
    peak_iterations: u32,
    cap_hits: u64,
    beyond: u64,
}

/// The curves at one excursion: value and first two derivatives of
/// `Bl(x)`, `Kms(x)` and `Le(x)` in SI units (`Kms(x)·x` is the force).
struct Curves {
    bl: f64,
    dbl: f64,
    k: f64,
    dk: f64,
    le: f64,
    dle: f64,
    ddle: f64,
}

impl Driver {
    /// A driver with the small-signal parameters `ts` and the
    /// large-signal curves `large`, driven 1 V per unit from an ideal
    /// voltage source.
    pub fn new(ts: ThieleSmall, large: LargeSignal) -> Self {
        let mut d = Self {
            ts,
            large,
            source_ohms: 0.0,
            volts: 1.0,
            h: 0.5 / 48_000.0,
            m: [[0.0; 4]; 4],
            n0: [0.0; 4],
            n1: [0.0; 4],
            s: [0.0; 4],
            u: 0.0,
            accel: 0.0,
            lr2: ts.l2 > 0.0 && ts.r2 > 0.0,
            algebraic: ts.le == 0.0,
            iterations: 0,
            peak_iterations: 0,
            cap_hits: 0,
            beyond: 0,
        };
        d.plan();
        d
    }

    /// The small-signal parameters.
    pub fn parameters(&self) -> &ThieleSmall {
        &self.ts
    }

    /// The large-signal curves.
    pub fn large_signal(&self) -> &LargeSignal {
        &self.large
    }

    /// Volts per unit of input (peak). Realtime-safe.
    pub fn set_volts(&mut self, volts: f64) {
        self.volts = volts;
    }

    /// The amplifier's output resistance, Ω (0: an ideal voltage
    /// source; a valve amplifier without feedback is several times
    /// `Re`). Realtime-safe; the states are kept.
    pub fn set_source_ohms(&mut self, ohms: f64) {
        self.source_ohms = ohms.max(0.0);
        self.plan();
    }

    /// The loop resistance `Re + Rg` and the LR-2 resistance in use.
    fn resistances(&self) -> (f64, f64) {
        (self.ts.re + self.source_ohms, if self.lr2 { self.ts.r2 } else { 0.0 })
    }

    /// The curves at `x`, held at their edges beyond `range`.
    #[inline]
    fn curves(&self, x: f64) -> Curves {
        let t = &self.ts;
        let range = self.large.range;
        let xe = x.clamp(-range, range);
        let inside = if xe == x { 1.0 } else { 0.0 };
        let (pb, db, _) = self.large.bl.eval(xe);
        let (pk, dk, _) = self.large.kms.eval(xe);
        let (pl, dl, ddl) = self.large.le.eval(xe);
        Curves {
            bl: t.bl * pb,
            dbl: t.bl * db * inside,
            k: pk / t.cms,
            dk: dk * inside / t.cms,
            le: t.le * pl,
            dle: t.le * dl * inside,
            ddle: t.le * ddl * inside,
        }
    }

    /// The coil's voltage balance `u − (Re + Rg)·i − u₂ − Bl·v` (its flux
    /// rate, `d(Le·i)/dt`) and the cone's force, at `s` and `u`.
    #[inline]
    fn rates(&self, s: &State, u: f64, c: &Curves) -> (f64, f64, f64) {
        let [i, i2, v, x] = *s;
        let (ret, r2) = self.resistances();
        let u2 = r2 * (i - i2);
        let electrical = u - ret * i - u2 - c.bl * v;
        let force = c.bl * i - self.ts.rms * v - c.k * x + 0.5 * i * i * c.dle;
        (electrical, u2, force)
    }

    /// The linear step's matrices at the current rate and source: the
    /// trapezoidal rule on the coil's flux `Le·i`, the LR-2 branch's
    /// flux `L2·i₂`, the momentum `Mms·v` and `x`; with `Le = 0` the
    /// coil's balance is held at the step's end instead.
    fn plan(&mut self) {
        let t = self.ts;
        let h = self.h;
        let (ret, r2) = self.resistances();
        // Flux/momentum form: E·ds/dt = A·s + B·u.
        let mut a = [[0.0; 4]; 4];
        a[0] = [-(ret + r2), r2, -t.bl, 0.0];
        if self.lr2 {
            a[1] = [r2, -r2, 0.0, 0.0];
        }
        a[2] = [t.bl, 0.0, -t.rms, -1.0 / t.cms];
        a[3] = [0.0, 0.0, 1.0, 0.0];
        let e = [t.le, if self.lr2 { t.l2 } else { 1.0 }, t.mms, 1.0];
        let mut lhs = [[0.0; 4]; 4];
        let mut rhs = [[0.0; 4]; 4];
        let (mut b0, mut b1) = ([0.0; 4], [0.0; 4]);
        for r in 0..4 {
            for c in 0..4 {
                let id = if r == c { e[r] } else { 0.0 };
                lhs[r][c] = id - h * a[r][c];
                rhs[r][c] = id + h * a[r][c];
            }
        }
        if self.algebraic {
            // 0 = u₁ + A₀·s₁: no memory of the step's start.
            lhs[0] = [-a[0][0], -a[0][1], -a[0][2], -a[0][3]];
            rhs[0] = [0.0; 4];
            b1[0] = 1.0;
        } else {
            b0[0] = h;
            b1[0] = h;
        }
        for c in 0..4 {
            let col = solve(lhs, [rhs[0][c], rhs[1][c], rhs[2][c], rhs[3][c]]);
            for r in 0..4 {
                self.m[r][c] = col[r];
            }
        }
        self.n0 = solve(lhs, b0);
        self.n1 = solve(lhs, b1);
    }

    /// One sample of `u` volts at the terminals; returns the cone's
    /// acceleration, m/s².
    #[inline]
    pub fn drive(&mut self, u: f64) -> f64 {
        let s0 = self.s;
        let s1 = if self.large.is_linear() {
            let mut s1 = [0.0; 4];
            for (r, out) in s1.iter_mut().enumerate() {
                let row = &self.m[r];
                *out = row[0] * s0[0] + row[1] * s0[1] + row[2] * s0[2] + row[3] * s0[3]
                    + self.n0[r] * self.u
                    + self.n1[r] * u;
            }
            s1
        } else {
            self.newton(s0, u)
        };
        self.s = s1;
        self.u = u;
        let c = self.curves(s1[3]);
        self.accel = self.rates(&s1, u, &c).2 / self.ts.mms;
        self.accel
    }

    /// The large-signal step: Newton's method on the trapezoidal rule
    /// in flux/momentum form,
    /// `Le(x₁)·i₁ − Le(x₀)·i₀ = h·(e₀ + e₁)`, `L2·Δi₂ = h·(u₂₀ + u₂₁)`,
    /// `Mms·Δv = h·(F₀ + F₁)`, `Δx = h·(v₀ + v₁)`.
    fn newton(&mut self, s0: State, u: f64) -> State {
        let t = self.ts;
        let h = self.h;
        let (ret, r2) = self.resistances();
        let c0 = self.curves(s0[3]);
        let (e0, u20, f0) = self.rates(&s0, self.u, &c0);
        let flux0 = c0.le * s0[0];
        // Predictor: explicit Euler on v and x, the coil held.
        let mut s1 = s0;
        s1[2] = s0[2] + 2.0 * h * f0 / t.mms;
        s1[3] = s0[3] + 2.0 * h * s0[2];
        let tol = [1e-12, 1e-12, 1e-12, 1e-15];
        let mut iterations = 0;
        let mut converged = false;
        while iterations < MAX_ITERATIONS {
            iterations += 1;
            let [i, i2, v, x] = s1;
            let c = self.curves(x);
            let (e1, u21, f1) = self.rates(&s1, u, &c);
            let mut g = [0.0; 4];
            let mut j = [[0.0; 4]; 4];
            if self.algebraic {
                g[0] = -e1;
                j[0] = [ret + r2, -r2, c.bl, c.dbl * v];
            } else {
                g[0] = c.le * i - flux0 - h * (e0 + e1);
                j[0] = [c.le + h * (ret + r2), -h * r2, h * c.bl, c.dle * i + h * c.dbl * v];
            }
            if self.lr2 {
                g[1] = t.l2 * (i2 - s0[1]) - h * (u20 + u21);
                j[1] = [-h * r2, t.l2 + h * r2, 0.0, 0.0];
            } else {
                g[1] = i2;
                j[1] = [0.0, 1.0, 0.0, 0.0];
            }
            g[2] = t.mms * (v - s0[2]) - h * (f0 + f1);
            j[2] = [
                -h * (c.bl + i * c.dle),
                0.0,
                t.mms + h * t.rms,
                -h * (c.dbl * i - (c.k + c.dk * x) + 0.5 * i * i * c.ddle),
            ];
            g[3] = x - s0[3] - h * (s0[2] + v);
            j[3] = [0.0, 0.0, -h, 1.0];
            let step = solve(j, g);
            let mut small = true;
            for k in 0..4 {
                s1[k] -= step[k];
                small &= step[k].abs() <= tol[k] * (1.0 + s1[k].abs());
            }
            if small {
                converged = true;
                break;
            }
        }
        if !converged {
            self.cap_hits += 1;
        }
        self.iterations = iterations;
        self.peak_iterations = self.peak_iterations.max(iterations);
        if s1[3].abs() > self.large.range {
            self.beyond += 1;
        }
        s1
    }

    /// The scale that puts a small signal's mass-controlled band at
    /// unity: `a ≈ Bl·u/(Mms·(Re + Rg))` there.
    fn normalisation(&self) -> f64 {
        let t = &self.ts;
        t.mms * (t.re + self.source_ohms) / (t.bl * self.volts)
    }

    /// The cone's acceleration per unit of input in the small-signal
    /// mass-controlled band, `Bl·volts/(Mms·(Re + Rg))`, m/s²: what the
    /// stage's output is normalised by.
    pub fn mass_band_acceleration(&self) -> f64 {
        1.0 / self.normalisation()
    }

    /// Voice-coil current, A.
    pub fn current(&self) -> f64 {
        self.s[0]
    }

    /// Cone velocity, m/s.
    pub fn velocity(&self) -> f64 {
        self.s[2]
    }

    /// Cone excursion (positive outward), m.
    pub fn excursion(&self) -> f64 {
        self.s[3]
    }

    /// Cone acceleration, m/s².
    pub fn acceleration(&self) -> f64 {
        self.accel
    }

    /// On-axis far-field pressure `distance` m away in a baffle (half
    /// space), Pa, at the propagation's retarded time (the delay `r/c`
    /// is the caller's).
    pub fn pressure(&self, distance: f64) -> f64 {
        AIR_DENSITY * self.ts.sd * self.accel / (TAU * distance)
    }

    /// Newton iterations the last sample took (0 when linear).
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    /// The most iterations any sample took since `reset`.
    pub fn peak_iterations(&self) -> u32 {
        self.peak_iterations
    }

    /// Samples whose Newton solve hit [`MAX_ITERATIONS`] since `reset`.
    pub fn cap_hits(&self) -> u64 {
        self.cap_hits
    }

    /// Samples spent beyond the large-signal curves' range since `reset`.
    pub fn beyond_range(&self) -> u64 {
        self.beyond
    }
}

impl Stage for Driver {
    fn prepare(&mut self, sample_rate: f32) {
        self.h = 0.5 / f64::from(sample_rate);
        self.plan();
        self.reset();
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
        self.u = 0.0;
        self.accel = 0.0;
        self.iterations = 0;
        self.peak_iterations = 0;
        self.cap_hits = 0;
        self.beyond = 0;
    }

    /// One sample of input (× [`set_volts`](Self::set_volts) volts) to
    /// normalised on-axis pressure.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let a = self.drive(f64::from(x) * self.volts);
        (a * self.normalisation()) as f32
    }

    fn tail(&self) -> Tail {
        // The slowest pole: the mechanical resonance's decay, to −120 dB.
        let t = &self.ts;
        let sigma = t.rms / (2.0 * t.mms) + t.bl * t.bl / (2.0 * t.mms * (t.re + self.source_ohms));
        let samples = (120.0 / 20.0 * core::f64::consts::LN_10) / sigma / (2.0 * self.h);
        Tail::Samples(samples.ceil() as u32)
    }
}

/// `a·x = b` by Gaussian elimination with partial pivoting.
#[inline]
pub(crate) fn solve(mut a: Matrix, mut b: State) -> State {
    for col in 0..4 {
        let mut p = col;
        for r in col + 1..4 {
            if a[r][col].abs() > a[p][col].abs() {
                p = r;
            }
        }
        a.swap(col, p);
        b.swap(col, p);
        let d = a[col][col];
        if d == 0.0 {
            continue;
        }
        for r in col + 1..4 {
            let f = a[r][col] / d;
            if f != 0.0 {
                for c in col..4 {
                    a[r][c] -= f * a[col][c];
                }
                b[r] -= f * b[col];
            }
        }
    }
    let mut x = [0.0; 4];
    for r in (0..4).rev() {
        let mut acc = b[r];
        for c in r + 1..4 {
            acc -= a[r][c] * x[c];
        }
        x[r] = if a[r][r] == 0.0 { 0.0 } else { acc / a[r][r] };
    }
    x
}
