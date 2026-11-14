//! A body heard at one point, as its modes: the plate's SIMD phasor
//! lanes ([`Lanes`]) driven by a list of modes in SI units, one pickup
//! calibrated to a unit-energy impulse response at 48 kHz (the same
//! continuous energy at every rate, as the plate's). The membrane and
//! the bar are this with their own mode sets.

use crate::kernel::node::Tail;
use crate::physical::lanes::{Lanes, BLOCK};
use crate::physical::plate::Output;
use crate::units::Seconds;

/// One mode of a body, in SI units: it rings at `√(natural² − sigma²)`
/// rad/s, decays at `sigma` 1/s, and an impulse of force at the drive
/// moves the pickup by `amplitude · e^(−σt) sin ωt / ω` (the shapes'
/// product over the modal mass).
#[derive(Copy, Clone, Debug, PartialEq)]
pub(in crate::physical) struct Ring {
    pub natural: f64,
    pub sigma: f64,
    pub amplitude: f64,
}

impl Ring {
    pub fn damped(&self) -> f64 {
        (self.natural * self.natural - self.sigma * self.sigma).max(0.0).sqrt()
    }
}

/// The modes run as Mathews–Smith phasors, one pickup.
#[derive(Clone, Debug, Default)]
pub(in crate::physical) struct Body {
    lanes: Lanes,
    modes: usize,
    scale: f64,
    slowest: f64,
}

impl Body {
    /// Room for `modes` modes (allocates).
    pub fn allocate(&mut self, modes: usize) {
        self.lanes.allocate(modes);
        self.modes = modes;
    }

    /// Set every mode (no allocation; at most the allocated count):
    /// rotations `r e^(jωT)` and the pickup's weights on `Re z`/`Im z`
    /// for `output` (the impulse-invariant derivatives of the plate),
    /// scaled so the impulse response carries unit energy at 48 kHz and
    /// the same continuous energy at every rate (`Σh² = 48 kHz / fs`):
    /// unit energy at each rate put the gain on `√fs`, +6 dB at 192 kHz
    /// (DSP.md R6).
    pub fn set(&mut self, rings: &[Ring], output: Output, sample_rate: f32) {
        let t = 1.0 / f64::from(sample_rate);
        let weights = |m: &Ring| -> (f64, f64) {
            let (w, s) = (m.damped(), m.sigma);
            if w <= 0.0 {
                return (0.0, 0.0);
            }
            let a = t * m.amplitude / w;
            match output {
                Output::Displacement => (0.0, a),
                Output::Velocity => (a * w, -a * s),
                Output::Acceleration => (-2.0 * a * s * w, a * (s * s - w * w)),
            }
        };
        let mut energy = 0.0;
        self.slowest = f64::INFINITY;
        for m in rings.iter().take(self.modes) {
            let (wr, wi) = weights(m);
            energy += (wr * wr + wi * wi) / (2.0 * (1.0 - (-2.0 * m.sigma * t).exp()));
            if m.damped() > 0.0 {
                self.slowest = self.slowest.min(m.sigma);
            }
        }
        // A sampled response's energy goes as T: calibrate at 48 kHz.
        let energy = energy * f64::from(sample_rate) / 48_000.0;
        self.scale = if energy > 0.0 && energy.is_finite() { 1.0 / energy.sqrt() } else { 0.0 };
        for i in 0..self.modes {
            match rings.get(i) {
                Some(m) => {
                    let r = (-m.sigma * t).exp();
                    let (s, c) = (m.damped() * t).sin_cos();
                    let (wr, wi) = weights(m);
                    self.lanes.set_rotation(i, (r * c) as f32, (r * s) as f32);
                    self.lanes.set_weights(i, [(wr * self.scale) as f32, (wi * self.scale) as f32, 0.0, 0.0]);
                }
                None => {
                    self.lanes.set_rotation(i, 0.0, 0.0);
                    self.lanes.set_weights(i, [0.0; 4]);
                }
            }
        }
    }

    /// The factor from SI units to the output.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn reset(&mut self) {
        self.lanes.reset();
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let (mut l, mut r) = ([0.0f32], [0.0f32; 0]);
        self.lanes.run::<false>(&[x], &mut l, &mut r);
        l[0]
    }

    /// In place, block by block; bit-identical to `tick`.
    pub fn process(&mut self, block: &mut [f32]) {
        let mut input = [0.0f32; BLOCK];
        let mut unused = [0.0f32; 0];
        let mut at = 0;
        while at < block.len() {
            let n = self.lanes.room().min(block.len() - at);
            input[..n].copy_from_slice(&block[at..at + n]);
            self.lanes.run::<false>(&input[..n], &mut block[at..at + n], &mut unused);
            at += n;
        }
    }

    /// Two of the slowest mode's T60s (−120 dB).
    pub fn tail(&self, sample_rate: f32) -> Tail {
        if self.modes == 0 || !self.slowest.is_finite() {
            Tail::None
        } else {
            Tail::from_time_constant(Seconds::new(1.0 / self.slowest), sample_rate)
        }
    }
}
