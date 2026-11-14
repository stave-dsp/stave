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
/// the ODE in H is rate independent, so both solve the same loop. The
/// bias dominates the field and repeats every cycle, so each sample's
/// Newton starts from the susceptibility the same sample of the last
/// bias cycle saw (the solution is the same; it takes fewer iterations).
///
/// # Verified
/// `crates/stave-dsp/src/physical/tape/tests.rs::bias_linearises_the_dead_zone`
/// (the paper's claims, measured), and through [`Chowdhury`](super::Chowdhury):
/// `crates/stave-test/tests/tape_matches_reference.rs::hysteresis_loop_against_chowdhury_fig_2`;
/// standalone at 16× from 44.1 to 192 kHz in DSP.md R6's sweep
/// (`crates/stave-test/tests/rate_range.rs`, `"Recording (TC-260, 16x)"`),
/// the same third harmonic of a hot 200 Hz tone at every rate.
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
/// the tape's saturation); rates from 12 times the bias frequency
/// (660 kHz for the TC-260), where the bias holds [`BIAS_SAMPLES`] a
/// cycle; below, down to twice the bias frequency, the count follows
/// the rate and so does the sound.
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
    /// Each step's mean susceptibility ΔM/ΔH over the last bias cycle, by
    /// its place in the cycle: the next cycle's Newton guesses.
    secants: Vec<f64>,
    at: usize,
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
            secants: Vec::new(),
            at: 0,
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
        let (m0, h0) = (self.core.magnetization(), self.core.field());
        let m = self.core.step_predicted(h, self.secants.get(self.at).copied());
        if let Some(secant) = self.secants.get_mut(self.at) {
            *secant = if h != h0 { (m - m0) / (h - h0) } else { f64::NAN };
            self.at = (self.at + 1) % self.secants.len();
        }
        (m * self.gain, h)
    }
}

/// Inner samples a bias cycle across the record side's valid range: the
/// hysteresis sees the same sampled bias whatever the rate, so its
/// harmonics are the same. The Jiles–Atherton loop is rate independent
/// (an ODE in H), so what it makes of the bias is set by the field's
/// path through a cycle — its samples — not by the bias's frequency; a
/// count that follows the frequency instead (12 at 705.6 kHz, 14 at 768,
/// 56 at 3.07 MHz) moves the third harmonic of a hot 200 Hz tone from
/// −55 to −71 dBc. A phase-continuous bias at the machine's exact
/// frequency holds that harmonic within 1.2 dB but folds the bias's
/// harmonics inharmonically into the band (the 13th of 55 kHz at
/// 720 kHz lands at 5 kHz, −25 dB under a −6 dBFS tone), so the count
/// is whole, and fixed.
pub const BIAS_SAMPLES: u32 = 14;

impl Recording {
    /// The bias frequency in use at `sample_rate`: [`BIAS_SAMPLES`] a
    /// cycle across the valid range (from 12 times the machine's
    /// frequency up: 50.4 kHz at 705.6, 54.9 at 768, 219 at 3.07 MHz,
    /// against the TC-260's 55), else the even count nearest the
    /// machine's frequency. Whole, so every harmonic the hysteresis
    /// makes of it folds onto another harmonic of it — never into the
    /// audio band; even, so the sampled bias keeps its half-wave symmetry
    /// and the symmetric loop makes no even harmonics (an odd count
    /// leaves H2 at −64 dBc).
    pub fn bias_hz(&self, sample_rate: f64) -> f64 {
        sample_rate / self.samples_per_bias(sample_rate)
    }

    /// Samples a bias cycle at `sample_rate`: [`set_bias_samples`](Self::set_bias_samples)'s,
    /// else [`BIAS_SAMPLES`] in the valid range and the even count
    /// nearest the machine's frequency below it.
    fn samples_per_bias(&self, sample_rate: f64) -> f64 {
        match self.bias_samples {
            Some(n) => f64::from(n),
            None if sample_rate >= 12.0 * self.machine.bias.hz => f64::from(BIAS_SAMPLES),
            None => 2.0 * (0.5 * sample_rate / self.machine.bias.hz).round().max(1.0),
        }
    }

    /// Hold the bias at `samples` (even) a cycle whatever the rate.
    pub fn set_bias_samples(&mut self, samples: u32) {
        self.bias_samples = Some(2 * (samples / 2).max(1));
    }
}

impl Stage for Recording {
    fn prepare(&mut self, sample_rate: f32) {
        let period = self.samples_per_bias(f64::from(sample_rate));
        self.increment = 1.0 / period;
        self.secants = vec![f64::NAN; period as usize];
        self.at = 0;
    }
    fn reset(&mut self) {
        self.core.reset();
        self.phase = 0.0;
        self.secants.iter_mut().for_each(|s| *s = f64::NAN);
        self.at = 0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.write(f64::from(x)).0 as f32
    }
}
