//! The mathematical mechanism behind a passive three-control amplifier tone
//! network.
//!
//! The network, unloaded: input through C1 to the treble pot's top and
//! through the slope resistor R4 to C2 and C3; the treble wiper is the
//! output; the bass pot runs from the treble pot's foot (where C2 lands)
//! to the mid pot's top; C3 lands on the mid pot's wiper; the mid pot's
//! foot is ground. Yeh & Smith's third-order transfer function for it
//! matches ngspice's AC analysis to 1e-11 dB (stave-test
//! `matches_ngspice`), realized by the bilinear transform.
//!
//! Products own their component values and control laws. This owns the
//! network's transfer function and its allocation-free digital
//! realization. It is deliberately not an "amp tone stack" assembly: a
//! Baxandall network, a four-control network, or an active equalizer is a
//! different mechanism rather than a mode of this type.

use crate::kernel::node::{Stage, Tail};

/// Component values for the family of passive treble/bass/midrange networks
/// described by Yeh and Smith (DAFx-06).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassiveToneStackComponents {
    pub treble_capacitance_farads: f64,
    pub bass_capacitance_farads: f64,
    pub midrange_capacitance_farads: f64,
    pub treble_potentiometer_ohms: f64,
    pub bass_potentiometer_ohms: f64,
    pub midrange_potentiometer_ohms: f64,
    pub slope_resistance_ohms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassiveToneControlPositions {
    pub treble: f64,
    pub bass: f64,
    pub midrange: f64,
}

impl PassiveToneControlPositions {
    pub const fn new(treble: f64, bass: f64, midrange: f64) -> Self {
        Self {
            treble,
            bass,
            midrange,
        }
    }
}

/// A third-order transfer function's coefficients, lowest power first.
#[derive(Clone, Copy, Debug, Default)]
struct Coefficients {
    numerator: [f64; 4],
    denominator: [f64; 4],
}

/// Concrete, allocation-free passive tone-stack processor.
#[derive(Clone)]
pub struct PassiveToneStack {
    components: PassiveToneStackComponents,
    sample_rate: f64,
    controls: PassiveToneControlPositions,
    /// The network function in s (analog), and its bilinear image in z.
    analog: Coefficients,
    coefficients: Coefficients,
    input_history: [f64; 3],
    output_history: [f64; 3],
}

impl PassiveToneStack {
    pub fn new(components: PassiveToneStackComponents) -> Self {
        let controls = PassiveToneControlPositions::new(0.5, 0.5, 0.5);
        let mut stack = Self {
            components,
            sample_rate: 48_000.0,
            controls,
            analog: Coefficients::default(),
            coefficients: Coefficients::default(),
            input_history: [0.0; 3],
            output_history: [0.0; 3],
        };
        stack.update_coefficients();
        stack
    }

    /// Update all three potentiometers as one circuit operation.
    pub fn set_controls(&mut self, controls: PassiveToneControlPositions) {
        debug_assert!(
            [controls.treble, controls.bass, controls.midrange]
                .iter()
                .all(|p| (0.0..=1.0).contains(p)),
            "pot positions are 0..=1: {controls:?}"
        );
        self.controls = controls;
        self.update_coefficients();
    }

    /// The network's own magnitude at `frequency_hz` (the analog transfer
    /// function, before discretization).
    pub fn circuit_magnitude_at(&self, frequency_hz: f64) -> f64 {
        let w = core::f64::consts::TAU * frequency_hz;
        let at = |c: &[f64; 4]| {
            // c0 + c1·s + c2·s² + c3·s³ at s = jω.
            let real = c[0] - c[2] * w * w;
            let imaginary = c[1] * w - c[3] * w * w * w;
            real.hypot(imaginary)
        };
        at(&self.analog.numerator) / at(&self.analog.denominator)
    }

    /// The digital filter's magnitude at one frequency, for staging.
    pub fn magnitude_at(&self, frequency_hz: f64) -> f64 {
        let omega = core::f64::consts::TAU * frequency_hz / self.sample_rate;
        let mut numerator = (0.0, 0.0);
        let mut denominator = (0.0, 0.0);
        for coefficient_index in 0..4 {
            let (sine, cosine) = (-(coefficient_index as f64) * omega).sin_cos();
            numerator.0 += self.coefficients.numerator[coefficient_index] * cosine;
            numerator.1 += self.coefficients.numerator[coefficient_index] * sine;
            denominator.0 += self.coefficients.denominator[coefficient_index] * cosine;
            denominator.1 += self.coefficients.denominator[coefficient_index] * sine;
        }
        let numerator_magnitude_squared = numerator.0 * numerator.0 + numerator.1 * numerator.1;
        let denominator_magnitude_squared =
            denominator.0 * denominator.0 + denominator.1 * denominator.1;
        (numerator_magnitude_squared / denominator_magnitude_squared.max(1e-30)).sqrt()
    }

    fn update_coefficients(&mut self) {
        // The equations retain the paper's c1-c3, r1-r4, t/l/m symbols so
        // this transcription can be checked directly against the derivation.
        let c1 = self.components.treble_capacitance_farads;
        let c2 = self.components.bass_capacitance_farads;
        let c3 = self.components.midrange_capacitance_farads;
        let r1 = self.components.treble_potentiometer_ohms;
        let r2 = self.components.bass_potentiometer_ohms;
        let r3 = self.components.midrange_potentiometer_ohms;
        let r4 = self.components.slope_resistance_ohms;
        let t = self.controls.treble;
        let l = self.controls.bass;
        let m = self.controls.midrange;
        let m2 = m * m;
        let r3_2 = r3 * r3;

        let b1 = t * c1 * r1 + m * c3 * r3 + l * (c1 * r2 + c2 * r2) + c1 * r3 + c2 * r3;
        let b2 = t * (c1 * c2 * r1 * r4 + c1 * c3 * r1 * r4)
            - m2 * (c1 * c3 * r3_2 + c2 * c3 * r3_2)
            + m * (c1 * c3 * r1 * r3 + c1 * c3 * r3_2 + c2 * c3 * r3_2)
            + l * (c1 * c2 * r1 * r2 + c1 * c2 * r2 * r4 + c1 * c3 * r2 * r4)
            + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
            + c1 * c2 * r1 * r3
            + c1 * c2 * r3 * r4
            + c1 * c3 * r3 * r4;
        let b3 = l * m * (c1 * c2 * c3 * r1 * r2 * r3 + c1 * c2 * c3 * r2 * r3 * r4)
            - m2 * (c1 * c2 * c3 * r1 * r3_2 + c1 * c2 * c3 * r3_2 * r4)
            + m * (c1 * c2 * c3 * r1 * r3_2 + c1 * c2 * c3 * r3_2 * r4)
            + t * c1 * c2 * c3 * r1 * r3 * r4
            - t * m * c1 * c2 * c3 * r1 * r3 * r4
            + t * l * c1 * c2 * c3 * r1 * r2 * r4;

        let a0 = 1.0;
        let a1 =
            c1 * r1 + c1 * r3 + c2 * r3 + c2 * r4 + c3 * r4 + m * c3 * r3 + l * (c1 * r2 + c2 * r2);
        let a2 = m * (c1 * c3 * r1 * r3 - c2 * c3 * r3 * r4 + c1 * c3 * r3_2 + c2 * c3 * r3_2)
            + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
            - m2 * (c1 * c3 * r3_2 + c2 * c3 * r3_2)
            + l * (c1 * c2 * r2 * r4 + c1 * c2 * r1 * r2 + c1 * c3 * r2 * r4 + c2 * c3 * r2 * r4)
            + c1 * c2 * r1 * r4
            + c1 * c3 * r1 * r4
            + c1 * c2 * r3 * r4
            + c1 * c2 * r1 * r3
            + c1 * c3 * r3 * r4
            + c2 * c3 * r3 * r4;
        let a3 = l * m * (c1 * c2 * c3 * r1 * r2 * r3 + c1 * c2 * c3 * r2 * r3 * r4)
            - m2 * (c1 * c2 * c3 * r1 * r3_2 + c1 * c2 * c3 * r3_2 * r4)
            + m * (c1 * c2 * c3 * r3_2 * r4 + c1 * c2 * c3 * r1 * r3_2
                - c1 * c2 * c3 * r1 * r3 * r4)
            + l * c1 * c2 * c3 * r1 * r2 * r4
            + c1 * c2 * c3 * r1 * r3 * r4;

        self.analog = Coefficients {
            numerator: [0.0, b1, b2, b3],
            denominator: [a0, a1, a2, a3],
        };
        let bilinear_scale = 2.0 * self.sample_rate;
        let bilinear_scale_squared = bilinear_scale * bilinear_scale;
        let bilinear_scale_cubed = bilinear_scale_squared * bilinear_scale;
        let numerator = [
            -b1 * bilinear_scale - b2 * bilinear_scale_squared - b3 * bilinear_scale_cubed,
            -b1 * bilinear_scale + b2 * bilinear_scale_squared + 3.0 * b3 * bilinear_scale_cubed,
            b1 * bilinear_scale + b2 * bilinear_scale_squared - 3.0 * b3 * bilinear_scale_cubed,
            b1 * bilinear_scale - b2 * bilinear_scale_squared + b3 * bilinear_scale_cubed,
        ];
        let denominator = [
            -a0 - a1 * bilinear_scale - a2 * bilinear_scale_squared - a3 * bilinear_scale_cubed,
            -3.0 * a0 - a1 * bilinear_scale
                + a2 * bilinear_scale_squared
                + 3.0 * a3 * bilinear_scale_cubed,
            -3.0 * a0 + a1 * bilinear_scale + a2 * bilinear_scale_squared
                - 3.0 * a3 * bilinear_scale_cubed,
            -a0 + a1 * bilinear_scale - a2 * bilinear_scale_squared + a3 * bilinear_scale_cubed,
        ];
        let inverse_leading_denominator = 1.0 / denominator[0];
        self.coefficients = Coefficients {
            numerator: numerator.map(|value| value * inverse_leading_denominator),
            denominator: denominator.map(|value| value * inverse_leading_denominator),
        };
    }
}

impl Stage for PassiveToneStack {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = f64::from(sample_rate);
        self.reset();
        self.update_coefficients();
    }

    fn reset(&mut self) {
        self.input_history = [0.0; 3];
        self.output_history = [0.0; 3];
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let coefficients = self.coefficients;
        let current_input = f64::from(input);
        let current_output = coefficients.numerator[0] * current_input
            + coefficients.numerator[1] * self.input_history[0]
            + coefficients.numerator[2] * self.input_history[1]
            + coefficients.numerator[3] * self.input_history[2]
            - coefficients.denominator[1] * self.output_history[0]
            - coefficients.denominator[2] * self.output_history[1]
            - coefficients.denominator[3] * self.output_history[2];
        self.input_history = [current_input, self.input_history[0], self.input_history[1]];
        self.output_history = [
            current_output,
            self.output_history[0],
            self.output_history[1],
        ];
        current_output as f32
    }

    /// The slowest of the three poles, to −120 dB.
    fn tail(&self) -> Tail {
        let [_, d1, d2, d3] = self.coefficients.denominator;
        Tail::from_pole(cubic_pole_radius(d1, d2, d3)).max(Tail::Samples(3))
    }
}

/// The largest root magnitude of `z³ + d1·z² + d2·z + d3`: one real root
/// by bisection inside the Cauchy bound, the other two from the deflated
/// quadratic.
fn cubic_pole_radius(d1: f64, d2: f64, d3: f64) -> f64 {
    let p = |z: f64| ((z + d1) * z + d2) * z + d3;
    let bound = 1.0 + d1.abs().max(d2.abs()).max(d3.abs());
    let (mut lo, mut hi) = (-bound, bound);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if (p(mid) < 0.0) == (p(lo) < 0.0) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let r = 0.5 * (lo + hi);
    let (b, c) = (d1 + r, d2 + r * (d1 + r));
    let disc = b * b - 4.0 * c;
    let quadratic = if disc < 0.0 {
        c.abs().sqrt()
    } else {
        let s = disc.sqrt();
        ((-b + s) * 0.5).abs().max(((-b - s) * 0.5).abs())
    };
    r.abs().max(quadratic)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_components() -> PassiveToneStackComponents {
        PassiveToneStackComponents {
            treble_capacitance_farads: 250e-12,
            bass_capacitance_farads: 100e-9,
            midrange_capacitance_farads: 47e-9,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: 1e6,
            midrange_potentiometer_ohms: 25e3,
            slope_resistance_ohms: 100e3,
        }
    }

    #[test]
    fn every_panel_corner_is_finite_and_stable() {
        for treble in [0.0, 1.0] {
            for bass in [0.0, 1.0] {
                for midrange in [0.0, 1.0] {
                    let mut stack = PassiveToneStack::new(reference_components());
                    stack.prepare(48_000.0);
                    stack.set_controls(PassiveToneControlPositions::new(treble, bass, midrange));
                    let mut peak = 0.0f32;
                    for sample_index in 0..48_000 {
                        let output = stack.tick(if sample_index == 0 { 1.0 } else { 0.0 });
                        assert!(output.is_finite());
                        peak = peak.max(output.abs());
                    }
                    assert!(peak < 2.0, "passive circuit escaped");
                }
            }
        }
    }

    #[test]
    fn controls_interact_in_one_network() {
        let mut stack = PassiveToneStack::new(reference_components());
        stack.set_controls(PassiveToneControlPositions::new(0.5, 0.5, 0.0));
        let treble_with_midrange_down = stack.magnitude_at(5_000.0);
        stack.set_controls(PassiveToneControlPositions::new(0.5, 0.5, 1.0));
        let treble_with_midrange_up = stack.magnitude_at(5_000.0);
        assert!((20.0 * (treble_with_midrange_up / treble_with_midrange_down).log10()).abs() > 0.5);
    }
}
