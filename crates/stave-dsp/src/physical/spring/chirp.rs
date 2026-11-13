//! The dispersive element of a spring: a cascade of stretched first-order
//! allpasses, Välimäki–Parker–Abel's "spectral delay filter".

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};

/// `M` stretched first-order allpasses, `A(z^k)^M` with
/// `A(z) = (a + z⁻¹)/(1 + a·z⁻¹)`: a chirp — frequency-dependent group
/// delay, unit magnitude.
///
/// # Models
/// The dispersion generator of Välimäki, Parker & Abel, "Parametric
/// Spring Reverberation Effect", JAES 58(7/8), 2010, eq. 3 in Parker,
/// "Efficient Dispersion Generation Structures for Spring Reverb
/// Emulation", EURASIP JASP 2011: its group delay is (Parker eq. 4)
/// `D(ω) = k·M·(1 − a²) / (1 + 2a·cos(ωk) + a²)` samples — for `a > 0`
/// rising from `kM(1 − a)/(1 + a)` at DC to `kM(1 + a)/(1 − a)` at
/// `ω = π/k`, the spring's transition frequency `fs/(2k)`; for `a < 0`
/// falling from DC (the wide-band chirps).
///
/// # Derivation
/// Each section in direct form II on a `k`-sample state,
/// `w[n] = x[n] − a·w[n−k]`, `y[n] = a·w[n] + w[n−k]`, the output
/// rewritten `a·x[n] + (1 − a²)·w[n−k]` so a sample's path through the
/// cascade is one multiply–add per section; the sections share one
/// circular index. The stretch is whole samples.
///
/// # Verified
/// `crates/stave-dsp/src/physical/spring/tests.rs::chirp_group_delay_is_the_published_dispersion`
/// (the group delay of the measured impulse response, by the phase
/// slope of its DFT, against `D(ω)` from DC to `0.95·π/k`, within 1e-5
/// of the peak delay, for both signs of `a` and four shapes) and
/// `crates/stave-dsp/src/physical/spring/tests.rs::chirp_is_allpass`
/// (magnitude flat within 1e-4 dB).
///
/// # Cost
/// `"Chirp (100 sections, k = 6)"` on the datasheet: one multiply–add
/// pair per section.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// `|a| < 1`; `k ≥ 1`; the impulse response lasts about the peak group
/// delay `kM(1 + |a|)/(1 − |a|)` samples.
#[derive(Clone, Debug, Default)]
pub struct Chirp {
    a: f32,
    /// `1 − a²`.
    b: f32,
    stretch: usize,
    sections: usize,
    state: Vec<f32>,
    index: usize,
}

impl Chirp {
    /// `sections` allpasses of coefficient `a`, each stretched to `stretch`
    /// samples (at least 1). Allocates its state here.
    pub fn new(sections: usize, stretch: usize, a: f32) -> Self {
        let stretch = stretch.max(1);
        let a = a.clamp(-0.999, 0.999);
        Self {
            a,
            b: 1.0 - a * a,
            stretch,
            sections,
            state: vec![0.0; sections * stretch],
            index: 0,
        }
    }

    /// The coefficient `a`.
    pub fn coefficient(&self) -> f32 {
        self.a
    }

    /// The stretch `k`, in samples.
    pub fn stretch(&self) -> usize {
        self.stretch
    }

    /// The number of sections `M`.
    pub fn sections(&self) -> usize {
        self.sections
    }

    /// The design group delay at `cycles` per sample (`f/fs`), in
    /// samples: Parker's eq. 4.
    pub fn group_delay(&self, cycles: f64) -> f64 {
        let a = f64::from(self.a);
        let k = self.stretch as f64;
        let w = core::f64::consts::TAU * cycles;
        k * self.sections as f64 * (1.0 - a * a) / (1.0 + 2.0 * a * (w * k).cos() + a * a)
    }

    /// The mean group delay over a period of the response, `k·M`
    /// samples (each section's phase falls 2π per period of `2π/k`).
    pub fn mean_delay(&self) -> f64 {
        (self.stretch * self.sections) as f64
    }
}

impl Stage for Chirp {
    fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = 0.0);
        self.index = 0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.state.len() < self.sections * self.stretch {
            return x; // unprepared: a wire
        }
        let (a, b) = (self.a, self.b);
        let mut v = x;
        let mut slot = self.index;
        for _ in 0..self.sections {
            let old = self.state[slot];
            self.state[slot] = denormal::flush(v - a * old);
            v = a * v + b * old;
            slot += self.stretch;
        }
        self.index += 1;
        if self.index >= self.stretch {
            self.index = 0;
        }
        v
    }

    fn tail(&self) -> Tail {
        // The chirp's peak group delay, then twice one section's ring
        // (its pole radius |a|^(1/k)) for the narrow band at the peak.
        let a = f64::from(self.a.abs());
        let peak = (self.mean_delay() * (1.0 + a) / (1.0 - a)).ceil() as u32;
        let ring = Tail::from_pole(a.powf(1.0 / self.stretch as f64));
        ring.then(ring).plus(peak)
    }
}
