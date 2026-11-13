//! The record head and the tape's magnetisation: head current to field
//! (with the bias), field to magnetisation by Jiles–Atherton.

use super::Machine;
use crate::circuit::devices::magnetic::JilesAthertonCore;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::kernel::node::Stage;

/// The record head writing to tape: the signal current plus the bias
/// current through the head, the deep-gap field they make, and the
/// tape's magnetisation under it. Runs at the oversampled rate the bias
/// needs ([`Chowdhury`](super::Chowdhury) wraps it); its output is the
/// magnetisation, normalised (`M / Ms` times the machine's playback
/// gain), bias and all — the playback side's lowpass removes the bias.
///
/// # Models
/// The record process of an analog tape machine (Chowdhury, DAFx-19,
/// §§2.1–2.2): the record head's field from its current (eq. 16, the
/// Karlqvist field at the gap's centre, `H = N·E·I/g`), a high-frequency
/// bias added to the signal current (eq. 28), and the tape's
/// ferromagnetic hysteresis (eq. 6, Jiles–Atherton).
///
/// # Derivation
/// Chowdhury's zero-dimensional model: the field is the deep-gap field
/// at the gap's centre (eq. 15), no spatial recording. The hysteresis is
/// [`circuit::devices::magnetic`](crate::circuit::devices::magnetic)'s
/// Jiles–Atherton, advanced by the trapezoidal rule in H with a Newton
/// solve per sample (Holters & Zölzer) where Chowdhury uses RK4 in time:
/// the ODE in H is rate independent, so both solve the same loop.
///
/// # Verified
/// `crates/stave-dsp/src/physical/tape/tests.rs::bias_linearises_the_dead_zone`
/// (the paper's claims, measured), and through [`Chowdhury`](super::Chowdhury):
/// `crates/stave-test/tests/tape_matches_reference.rs::hysteresis_loop_against_chowdhury_fig_2`.
///
/// # Cost
/// Datasheet `"Chowdhury tape (TC-260, 7.5 ips)"`, which runs it 15× at 48 kHz.
///
/// # Aliasing
/// Its own rate must carry the bias (55 kHz) and the products of the
/// hysteresis: run it at 700 kHz or more ([`Chowdhury`](super::Chowdhury)
/// picks the factor).
///
/// # Valid ranges
/// Inputs to a few times full scale (the field stays within a few times
/// the tape's saturation); a rate above twice the bias frequency.
#[derive(Clone)]
pub struct Recording {
    machine: Machine,
    core: JilesAthertonCore,
    /// Field per unit of input, A/m.
    signal_field: f64,
    /// The bias's peak field, A/m.
    bias_field: f64,
    phase: f64,
    increment: f64,
    bias_samples: Option<u32>,
    gain: f64,
}

impl Recording {
    pub fn new(machine: Machine) -> Self {
        let mut r = Self {
            machine,
            core: JilesAthertonCore::new(machine.formulation.magnetic, NewtonSettings::DEFAULT),
            signal_field: 0.0,
            bias_field: 0.0,
            phase: 0.0,
            increment: 0.0,
            bias_samples: None,
            gain: 0.0,
        };
        r.configure();
        r.prepare(720_000.0);
        r
    }

    fn configure(&mut self) {
        let m = &self.machine;
        let per_amp = m.record.field_per_amp();
        self.signal_field = m.drive_amps * per_amp;
        self.bias_field = m.bias.ratio * m.drive_amps * per_amp;
        self.gain = m.playback_gain / m.formulation.magnetic.saturation;
        self.core.set_model(m.formulation.magnetic);
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// New bias gain (bias current over the full-scale signal current).
    pub fn set_bias(&mut self, ratio: f64) {
        self.machine.bias.ratio = ratio.max(0.0);
        self.configure();
    }

    /// The field at full-scale input, A/m.
    pub fn signal_field(&self) -> f64 {
        self.signal_field
    }

    /// The Newton account of the hysteresis solves.
    pub fn stats(&self) -> NewtonStats {
        self.core.stats()
    }

    /// One sample in f64: input (full scale 1) to normalised
    /// magnetisation, and the field it was written with.
    #[inline]
    pub fn write(&mut self, x: f64) -> (f64, f64) {
        let bias = self.bias_field * (std::f64::consts::TAU * self.phase).cos();
        self.phase += self.increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let h = self.signal_field * x + bias;
        (self.core.step(h) * self.gain, h)
    }
}

impl Recording {
    /// The bias frequency in use at `sample_rate`: the machine's,
    /// moved (by under 8% at 700 kHz) to an even number of samples a
    /// cycle. Whole, so every harmonic the hysteresis makes of it folds
    /// onto another harmonic of it — never into the audio band (at
    /// 720 kHz a 55 kHz bias's 13th harmonic would fold to 5 kHz); even,
    /// so the sampled bias keeps its half-wave symmetry and the
    /// symmetric loop makes no even harmonics (an odd count leaves H2 at
    /// −64 dBc).
    pub fn bias_hz(&self, sample_rate: f64) -> f64 {
        sample_rate / self.samples_per_bias(sample_rate)
    }

    /// Samples a bias cycle at `sample_rate`: [`set_bias_samples`](Self::set_bias_samples)'s,
    /// else the even count nearest the machine's frequency.
    fn samples_per_bias(&self, sample_rate: f64) -> f64 {
        match self.bias_samples {
            Some(n) => f64::from(n),
            None => 2.0 * (0.5 * sample_rate / self.machine.bias.hz).round().max(1.0),
        }
    }

    /// Hold the bias at `samples` (even) a cycle whatever the rate, so
    /// the hysteresis sees the same sampled bias at every host rate
    /// ([`Chowdhury`](super::Chowdhury) holds 14).
    pub fn set_bias_samples(&mut self, samples: u32) {
        self.bias_samples = Some(2 * (samples / 2).max(1));
    }
}

impl Stage for Recording {
    fn prepare(&mut self, sample_rate: f32) {
        self.increment = 1.0 / self.samples_per_bias(f64::from(sample_rate));
    }
    fn reset(&mut self) {
        self.core.reset();
        self.phase = 0.0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.write(f64::from(x)).0 as f32
    }
}
