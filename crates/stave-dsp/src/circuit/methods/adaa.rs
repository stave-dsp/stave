//! Antiderivative anti-aliasing inside a circuit's solve (DSP.md A1):
//! the stateful extension of the memoryless kernel
//! [`behavioral::adaa`](crate::behavioral::adaa) has, for nonlinearities
//! that sit in a feedback loop with the circuit's capacitors.
//!
//! # Derivation
//!
//! First-order ADAA (Parker, Zavalishin & Le Bivic 2016) replaces a
//! nonlinearity's value at a sample with its mean over the straight line
//! from the previous input to this one: a rectangular kernel convolved
//! with the linearly interpolated input, which suppresses what the
//! nonlinearity folds back. It also delays by half a sample, which a
//! feedback loop cannot absorb. Holters (DAFx 2019) makes it work in a
//! state space anyway:
//!
//! - **the nonlinearity is the solution of the delay-free loop**, not a
//!   device law: for the nodal DK method the map from the linear
//!   prediction `p` to the device outputs `q(p)` that solve
//!   `v = p + K·q(v)`; for a wave-digital root, from the arriving wave
//!   `a` to the reflected one;
//! - **the rest of the loop is delayed to match**: the states and the
//!   inputs enter the update as `½·(x[n−1] + x[n−2])` and
//!   `½·(u[n] + u[n−1])`, the averaging filter the kernel is for a linear
//!   element, so every term lands at `n − ½`;
//! - **the extra half sample is designed out**: a loop delay of 1.5
//!   samples is one sample at two thirds of the rate, so the linear part
//!   is discretised for the step `1.5·T`
//!   ([`Antialiasing::design_step_scale`]). Albertini, Bernardini & Sarti
//!   (DAFx 2020; JAES 2021) do the same for wave digital filters: the
//!   leaves' waves are averaged and the capacitors' port resistances use
//!   `1.5·T`.
//!
//! For a linear circuit the result is Holters' eq. (13):
//! `H̃(z) = ½(1 + z⁻¹)·H(ζ)` with `ζ⁻¹ = ½(z⁻¹ + z⁻²)` — the original
//! response on a curve just outside the unit circle, so a stable circuit
//! stays stable; the response bends above about a third of the rate,
//! which mild oversampling keeps out of band. Linear interpolation is
//! also the stable choice of interpolator inside a loop (Zheleznov &
//! Bilbao, DAFx 2024).
//!
//! ## The kernel through the solve, in closed form
//!
//! Where the solve is a one-port — one controlling voltage `v`, every
//! device output entering the network through the same port as one
//! current `i(v)` — the prediction is `p = v − κ·i(v)` (for a wave-digital
//! root `a = v + R·i(v)`, so `κ = −R`). The kernel needs the mean of
//! `i` over the straight line in `p`, and changing the integration
//! variable to `v` (`dp = (1 − κ·i′(v))·dv`) gives it from the device's
//! own antiderivative `I(v) = ∫ i dv` at the two solved voltages:
//!
//! ```text
//!     ∫ i dp = I(v) − κ·i(v)²/2
//!     ĩ = (Î − κ·s·ī) / (1 − κ·s)        Î = ΔI/Δv, s = Δi/Δv, ī = (i₀ + i₁)/2
//! ```
//!
//! [`one_port_mean`] evaluates the second line. Each divided difference
//! is formed in closed form without subtracting nearly equal numbers
//! (`expm1`, `sinh` of the half difference), so `v₀ ≈ v₁` is not a
//! special case: the ill-conditioned quotient of the textbook form never
//! appears, and the formula is exact for any `Δv`.
//!
//! ## Several controlling voltages
//!
//! With more than one controlling voltage (a triode, a transistor, a
//! diode pair with a series resistance on each diode) the solution map
//! has no scalar antiderivative: its mean over the line is a line
//! integral of a vector field that is not a gradient, so neither a
//! device's closed form nor a table of a device law's antiderivative
//! (a Koren plate law, a Gummel–Poon collector) gives it. The DK method
//! evaluates that line integral by Gauss–Lobatto quadrature
//! ([`LOBATTO`]): the two ends are this sample's and the last sample's
//! solves, each interior node one more warm-started solve.
//!
//! `Circuit<Wdf>`'s R-type root faces the same `v = p + K·q(v)` and takes
//! the same mean, with Albertini et al.'s synchronization on its trees:
//! it matches the DK method's ADAA to the solves' tolerance. The
//! hand-built wave-digital clipper's pair root takes the closed form
//! directly ([`wdf::adaa`](crate::circuit::methods::wdf::adaa)).
//!
//! ## What is not here
//!
//! - **Second order.** Its kernel delays by a whole sample, so the linear
//!   part would be designed for `2·T` and every state would enter as
//!   `x[n−2]` (Albertini et al.'s `H₂(z) = z⁻¹`): the even and odd
//!   samples become two half-rate circuits coupled only through the
//!   kernel, and the one-port's closed form needs the second
//!   antiderivative of the solution map. Not built.
//! - **Tables of a reduced scalar prediction** (Holters' Tube Screamer,
//!   whose three diodes see one linear combination of the states): the
//!   solution map tabulated along that combination, with its
//!   antiderivative, would make such circuits closed form at one solve
//!   a sample. It waits on DSP.md N3's solution tables.
//!
//! # Measured
//!
//! `crates/stave-dsp/tests/aliasing_budgets.rs` (the `*_holds_its_alias_budgets`
//! circuit tests) holds every circuit with and without ADAA at 1×, 2× and
//! 4×; at 44.1 kHz, the inharmonic energy below 20 kHz, dB re the
//! fundamental:
//!
//! | circuit | mean | 1× | 1× ADAA | 2× | 2× ADAA | 4× | 4× ADAA |
//! |---|---|---|---|---|---|---|---|
//! | WDF diode clipper ×10 | closed form | −9.9 | −28.4 | −24.4 | −39.3 | −43.3 | −64.6 |
//! | DK op-amp clipper | closed form | −14.4 | −28.4 | −26.8 | −41.2 | −35.7 | −69.2 |
//! | DK diode clipper ×10 (RS) | quadrature | −9.9 | −24.6 | −24.4 | −42.6 | −43.6 | −57.5 |
//! | DK common cathode (2 V) | quadrature | −32.2 | −48.9 | −68.7 | −98.9 | −132.2 | −133.6 |
//! | DK Fuzz Face (0.5 V) | quadrature | −5.1 | −5.9 | −9.3 | −14.1 | −17.0 | −21.1 |
//! | DK Tube Screamer (0.5 V) | quadrature | −13.7 | −17.6 | −25.7 | −28.1 | −32.1 | −39.4 |
//!
//! Closed form is the win: one solve a sample, and at 4× the op-amp
//! clipper sits 21 dB below plain 8× (−47.6), the WDF clipper level with
//! it (−65.6). Quadrature always lowers the aliasing, but its extra
//! solves cost more than oversampling to the same level: the Tube
//! Screamer at 4× with ADAA (eight solves a host sample) reaches −39.4
//! where plain 8× (eight solves) reaches −44.8. For a multi-port circuit,
//! oversample.
//!
//! In CPU (the datasheet's reference machine, ns/sample at 48 kHz on its
//! noise): the op-amp clipper at 1× with ADAA costs 691 against 1117 for
//! plain 2× (and aliases 1.6 dB less), at 4× with ADAA 2236 against 3223
//! for plain 8× (21.6 dB less); the WDF clipper 298 against 494 at the
//! first pair, 996 against 1430 at the last (within 1 dB). The Tube
//! Screamer by quadrature: 3817 at 1× with ADAA, 3953 for plain 2×, which
//! aliases 8 dB less. (The datasheet's `Circuit<Dk>` diode clipper has no
//! series resistance, so it is closed form there.)
//!
//! ADAA's output is the circuit half a sample late, and its averaging
//! filter `½(1 + z⁻¹)` dips the top of the band (`cos(ωT/2)`: −2.0 dB at
//! 10 kHz at 48 kHz, −0.5 dB at 2×); `crates/stave-test/tests/adaa_matches_ngspice.rs`
//! holds the in-band error against ngspice.
//!
//! ```
//! use stave_dsp::circuit::methods::adaa::one_port_mean;
//!
//! // A resistor of 1 kΩ is a linear one-port: its mean current is the
//! // current at the mean voltage, whatever the loop (κ) around it.
//! let (g, v0, v1) = (1e-3, 0.2, 0.6);
//! let mean_antiderivative = g * 0.5 * (v0 + v1);
//! let mean = one_port_mean(mean_antiderivative, g, 0.5 * g * (v0 + v1), -2.2e3);
//! assert!((mean - g * 0.4).abs() < 1e-15);
//! ```

/// How a circuit method anti-aliases its nonlinearities: a setting of
/// the method ([`Dk::set_antialiasing`](crate::circuit::methods::dk::Dk::set_antialiasing),
/// [`Wdf::set_antialiasing`](crate::circuit::methods::wdf::Wdf::set_antialiasing),
/// [`DiodeClipper::set_antialiasing`](crate::circuit::topologies::diode_clipper::DiodeClipper::set_antialiasing)),
/// not a model. Holters (DAFx 2019) for state spaces, Albertini,
/// Bernardini & Sarti (DAFx 2020, JAES 2021) for wave digital filters
/// (module docs). Above a third of the rate ADAA bends the linear
/// response, so pair [`Adaa`](Self::Adaa) with 2× oversampling where
/// the top octave matters.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Antialiasing {
    /// None of its own (the default): run the circuit oversampled.
    #[default]
    None,
    /// First-order stateful ADAA: the solve's outputs averaged over the
    /// line from the last sample's prediction to this one's, the linear
    /// part delayed by half a sample and designed for `1.5·T`.
    Adaa,
}

impl Antialiasing {
    /// The factor the linear part's discretisation step is stretched by:
    /// the kernel's half-sample delay plus the loop's one sample is one
    /// sample at `1/1.5` of the rate.
    pub const fn design_step_scale(self) -> f64 {
        match self {
            Self::None => 1.0,
            Self::Adaa => 1.5,
        }
    }
}

/// The mean of a one-port's current over the straight line between two
/// solves of `p = v − κ·i(v)`: `mean_antiderivative` is `ΔI/Δv`,
/// `secant` is `Δi/Δv` (the conductance where `Δv = 0`), `mean_current`
/// is `(i₀ + i₁)/2` (module docs). A wave-digital root solving
/// `a = v + R·i(v)` passes `κ = −R`.
#[inline]
pub fn one_port_mean(mean_antiderivative: f64, secant: f64, mean_current: f64, kappa: f64) -> f64 {
    let loop_gain = 1.0 - kappa * secant;
    if loop_gain.abs() < 1e-12 {
        // Only an active one-port (negative resistance) gets here.
        return mean_current;
    }
    (mean_antiderivative - kappa * secant * mean_current) / loop_gain
}

/// Gauss–Lobatto rules on `[0, 1]`, by node count: `(nodes, weights)`.
/// Both ends are nodes, so a stateful kernel reuses the last sample's
/// solve and this one's; `n` nodes integrate polynomials of degree
/// `2n − 3` exactly.
pub const LOBATTO: [(&[f64], &[f64]); 3] = [
    (&[0.0, 0.5, 1.0], &[1.0 / 6.0, 4.0 / 6.0, 1.0 / 6.0]),
    (
        &[0.0, 0.276_393_202_250_021, 0.723_606_797_749_979, 1.0],
        &[1.0 / 12.0, 5.0 / 12.0, 5.0 / 12.0, 1.0 / 12.0],
    ),
    (
        &[0.0, 0.172_673_164_646_011_4, 0.5, 0.827_326_835_353_988_6, 1.0],
        &[1.0 / 20.0, 49.0 / 180.0, 16.0 / 45.0, 49.0 / 180.0, 1.0 / 20.0],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::devices::junction::sinhc;

    /// The closed form is the mean over the line in `p`: checked against
    /// a fine midpoint rule on a diode pair behind 2.2 kΩ (p = v + R·i).
    #[test]
    fn the_one_port_mean_is_the_mean_over_the_prediction() {
        let (is, k, r) = (2.52e-9, 1.752 * 0.025_852, 2.2e3);
        let i = |v: f64| 2.0 * is * (v / k).sinh();
        let solve = |p: f64| {
            // p = v + R·i(v): bisection is plenty for a test.
            let (mut lo, mut hi) = (-2.0f64, 2.0f64);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid + r * i(mid) < p {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            0.5 * (lo + hi)
        };
        for (p0, p1) in [(-3.0, 4.0), (0.1, 0.1 + 1e-9), (0.6, 0.9), (5.0, -5.0)] {
            let n = 20_000;
            let reference: f64 = (0..n)
                .map(|j| i(solve(p0 + (p1 - p0) * (j as f64 + 0.5) / n as f64)))
                .sum::<f64>()
                / n as f64;
            let (v0, v1) = (solve(p0), solve(p1));
            let (vm, h) = (0.5 * (v0 + v1), 0.5 * (v1 - v0) / k);
            let mean = 2.0 * is * (vm / k).sinh() * sinhc(h);
            let secant = 2.0 * is / k * (vm / k).cosh() * sinhc(h);
            let got = one_port_mean(mean, secant, 0.5 * (i(v0) + i(v1)), -r);
            let scale = reference.abs().max(1e-9);
            assert!((got - reference).abs() < 1e-5 * scale + 1e-12, "{p0}..{p1}: {got} vs {reference}");
        }
    }

    #[test]
    fn lobatto_rules_integrate_their_degree() {
        for (n, (nodes, weights)) in LOBATTO.iter().enumerate() {
            let degree = 2 * (n as i32 + 3) - 3;
            for d in 0..=degree {
                let sum: f64 = nodes.iter().zip(*weights).map(|(x, w)| w * x.powi(d)).sum();
                assert!((sum - 1.0 / f64::from(d + 1)).abs() < 1e-14, "{} nodes, x^{d}", n + 3);
            }
        }
    }
}
