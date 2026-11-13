//! A circuit's own noise (DSP.md H9), and sources held from outside —
//! a supply rail fed by a power-supply model, so its ripple and sag
//! reach the circuit through the netlist.
//!
//! # Noise
//!
//! Each noise source is one more independent input of the state space
//! (a column of `Bu`), so it reaches the probe through the circuit's own
//! transfer, is corrected with the matrices when a part moves (N6) and
//! is averaged like any input under ADAA. Off by default: a circuit
//! without [`Dk::set_noise`] has no noise columns, costs nothing more and
//! is bit-identical to one built before H9.
//!
//! - **Thermal (Johnson–Nyquist).** Every resistor `R` — the netlist's,
//!   and a device's ohmic terminal resistances — gets a parallel current
//!   source of one-sided PSD `4kT/R` A²/Hz; a variable resistor (an MNA
//!   branch) a series voltage of `4kTR` V²/Hz at its present value.
//! - **Shot (Schottky).** Every diode's junction gets a parallel current
//!   of PSD `2q|I|`, `I` its current at the last sample.
//!
//! A white sequence of variance `σ² = S·f_s/2` has the one-sided PSD `S`
//! over `0 … f_s/2`, so each source draws standard normals (seeded,
//! [`SplitMix64`]) scaled by `√(S·f_s/2)`. The trapezoidal rule maps the
//! analog transfer `Z(jΩ)` from a source to the probe onto the discrete
//! frequency `ω` with `Ω = (2/T)·tan(ωT/2)`, so the output PSD is
//! exactly `S·|Z(jΩ(ω))|²` — the analytic noise of the netlist, warped
//! as its frequency response is ([`Dk::analog_response`]).
//!
//! Verified (`crates/stave-test/tests/noise_matches_theory.rs`): an RC
//! lowpass's output PSD within 0.3 dB a 16-bin band of `4kTR/(1 +
//! (ΩRC)²)` to `f_s/4` (measured 0.20) and its variance within 1.5 % of
//! the warped integral (0.18 %; 9 % under `kT/C`, the band above `f_s/2`
//! the discrete circuit lacks); a variable resistor's at 10 kΩ and after
//! a move to 47 kΩ the same; a diode at 0.445 mA within 0.3 dB of
//! `2qI·|Z|²` (0.16) and 1 % in variance (0.13 %); off is the circuit
//! without noise, bit for bit, and a reset replays the same noise.
//! Cost: datasheet `"Circuit<Dk> tone stack, thermal noise"` beside
//! `"Circuit<Dk> tone stack"`.
//!
//! Not modelled: flicker (1/f) noise, the BJT's and FET's channel and
//! junction noise (their ohmic resistances are thermal sources), a
//! tube's shot noise.

use super::devices::SHUNT;
use super::{Dk, Input};
use crate::circuit::methods::dk::Phasor;
use crate::circuit::netlist::NetlistError;
use crate::primitives::random::SplitMix64;

/// Boltzmann's constant, J/K (SI exact).
pub const BOLTZMANN: f64 = 1.380_649e-23;
/// The elementary charge, C (SI exact).
pub const ELECTRON_CHARGE: f64 = 1.602_176_634e-19;

/// Which noise a circuit makes, at what temperature, from what seed.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Noise {
    pub kelvin: f64,
    pub seed: u64,
    pub thermal: bool,
    pub shot: bool,
}

impl Noise {
    /// Thermal and shot noise at 27 °C (SPICE's nominal temperature).
    pub const fn room(seed: u64) -> Self {
        Self {
            kelvin: 300.15,
            seed,
            thermal: true,
            shot: true,
        }
    }

    pub const fn thermal_only(mut self) -> Self {
        self.shot = false;
        self
    }

    pub const fn shot_only(mut self) -> Self {
        self.thermal = false;
        self
    }
}

/// What makes a site's noise.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Site {
    /// A fixed resistor: a current, `4kT/R`.
    Thermal { ohms: f64 },
    /// A variable resistor's branch: a series voltage, `4kT·R(now)`.
    Branch { branch: usize },
    /// A junction: a current, `2q·|I|`, `I` the device's output
    /// `q[output]` plus the method's conditioning shunt across its
    /// control `v[control.0]·control.1`.
    Shot { output: usize, control: (usize, f64) },
}

/// A place noise enters: its kind and its `Bu` column (rows, signs).
#[derive(Clone, Debug)]
pub(crate) struct NoiseSite {
    pub element: String,
    pub site: Site,
    pub column: Vec<(usize, f64)>,
}

/// The noise in effect: its sites (indices into `Dk::noise_sites`), the
/// fixed thermal sources' scales, and the stream.
#[derive(Clone, Debug)]
pub(crate) struct NoiseState {
    pub config: Noise,
    pub active: Vec<usize>,
    pub scale: Vec<f64>,
    /// `2kT·f_s` (thermal variance per ohm of a branch) and `q·f_s`
    /// (shot variance per amp), at the prepared rate.
    pub thermal_per_ohm: f64,
    pub shot_per_amp: f64,
    pub rng: SplitMix64,
}

impl NoiseState {
    /// Scales for `sample_rate` (and the stream back to its seed).
    pub fn prepare(&mut self, sites: &[NoiseSite], sample_rate: f64) {
        let kt = BOLTZMANN * self.config.kelvin;
        self.thermal_per_ohm = 2.0 * kt * sample_rate;
        self.shot_per_amp = ELECTRON_CHARGE * sample_rate;
        for (scale, &s) in self.scale.iter_mut().zip(&self.active) {
            *scale = match sites[s].site {
                // Variance (4kT/R)·f_s/2.
                Site::Thermal { ohms } => (2.0 * kt * sample_rate / ohms).sqrt(),
                Site::Branch { .. } | Site::Shot { .. } => 0.0,
            };
        }
        self.rng = SplitMix64::new(self.config.seed);
    }

    /// The stream back to its seed.
    pub fn restart(&mut self) {
        self.rng = SplitMix64::new(self.config.seed);
    }

    /// Source `j`'s next value.
    #[inline]
    pub fn draw(&mut self, j: usize, sites: &[NoiseSite], values: &[f64], q: &[f64], v: &[f64]) -> f64 {
        let sigma = match sites[self.active[j]].site {
            Site::Thermal { .. } => self.scale[j],
            Site::Branch { branch } => (self.thermal_per_ohm * values[branch].max(0.0)).sqrt(),
            Site::Shot { output, control } => {
                let amps = q[output] + SHUNT * control.1 * v[control.0];
                (self.shot_per_amp * amps.abs()).sqrt()
            }
        };
        sigma * self.rng.next_normal()
    }
}

/// An independent source held from outside the circuit, sample by
/// sample ([`Dk::set_supply`]): a power rail driven by a supply model.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Supply(pub(crate) usize);

impl Dk {
    /// Make the circuit's thermal and shot noise (module docs), or none.
    /// Rebuilds the input columns and re-prepares at the current rate, so
    /// call it at setup, not on the audio thread; a reset restarts the
    /// noise from its seed, so a run reproduces.
    pub fn set_noise(&mut self, noise: Option<Noise>) {
        let base = self.base_inputs;
        let (n, old) = (self.n, self.inputs.len());
        let active: Vec<usize> = match noise {
            None => Vec::new(),
            Some(cfg) => (0..self.noise_sites.len())
                .filter(|&s| match self.noise_sites[s].site {
                    Site::Thermal { .. } | Site::Branch { .. } => cfg.thermal,
                    Site::Shot { .. } => cfg.shot,
                })
                .collect(),
        };
        let nu = base + active.len();
        let mut bu = vec![0.0; n * nu];
        for r in 0..n {
            bu[r * nu..r * nu + base].copy_from_slice(&self.bu[r * old..r * old + base]);
        }
        for (j, &s) in active.iter().enumerate() {
            for &(row, x) in &self.noise_sites[s].column {
                bu[row * nu + base + j] += x;
            }
        }
        self.bu = bu;
        self.inputs.truncate(base);
        self.input_ac.truncate(base);
        self.source_names.truncate(base);
        self.held.truncate(base);
        for (j, &s) in active.iter().enumerate() {
            self.inputs.push(Input::Noise(j));
            self.input_ac.push(0.0);
            self.source_names.push(format!("noise {}", self.noise_sites[s].element));
            self.held.push(None);
        }
        let (nx, nv, nq) = (self.x.len(), self.controls.len(), self.nq);
        self.u = vec![0.0; nu];
        self.du = vec![0.0; nv * nu];
        self.au = vec![0.0; nx * nu];
        self.eu = vec![0.0; nu];
        self.adaa = super::adaa::State::new(nx, nu, nv, nq);
        self.noise = noise.map(|config| NoiseState {
            config,
            scale: vec![0.0; active.len()],
            active,
            thermal_per_ohm: 0.0,
            shot_per_amp: 0.0,
            rng: SplitMix64::new(config.seed),
        });
        self.prepare(self.sample_rate);
    }

    /// The noise in effect.
    pub fn noise(&self) -> Option<Noise> {
        self.noise.as_ref().map(|n| n.config)
    }

    /// How many noise sources the circuit has with `noise` on: every
    /// resistor and ohmic resistance (thermal), every diode (shot).
    pub fn noise_sources(&self) -> usize {
        self.noise.as_ref().map_or(0, |n| n.active.len())
    }

    /// The independent source called `name`, to hold from outside.
    pub fn supply(&self, name: &str) -> Result<Supply, NetlistError> {
        let name = name.to_ascii_lowercase();
        self.source_names[..self.base_inputs]
            .iter()
            .position(|n| *n == name)
            .map(Supply)
            .ok_or(NetlistError::UnknownElement { name })
    }

    /// Hold `supply` at `volts` (amps, for a current source) from the
    /// next sample on, in place of its netlist signal. Realtime. The
    /// operating point stays the one solved at the netlist's value: a
    /// held rail far from it settles through the circuit's dynamics.
    #[inline]
    pub fn set_supply(&mut self, supply: Supply, volts: f64) {
        if let Some(h) = self.held.get_mut(supply.0) {
            *h = Some(volts);
        }
    }

    /// Back to the netlist's own signal.
    pub fn release_supply(&mut self, supply: Supply) {
        if let Some(h) = self.held.get_mut(supply.0) {
            *h = None;
        }
    }

    /// The small-signal response from `supply` to the probe at `hz`,
    /// analog (as [`analog_response`](Self::analog_response)): how much of
    /// a rail's ripple reaches the output.
    pub fn supply_response(&self, supply: Supply, hz: f64) -> Option<Phasor> {
        let angle = 2.0 * (std::f64::consts::PI * hz * self.step).atan();
        self.response_from(supply.0, 1.0, angle)
    }
}
