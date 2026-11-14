//! Magnetic tape: a reel-to-reel machine's record head, the tape's
//! hysteresis, the play head's losses and the transport's wow and
//! flutter (DSP.md E8), after Chowdhury's physical model (DAFx-19).
//!
//! **Why `physical`.** A tape machine's sound here comes from magnetic
//! recording physics — the head's field (Karlqvist), the tape's
//! hysteresis (Jiles–Atherton), the reciprocity integral that gives the
//! play head's spacing, thickness and gap losses (Bertram), the
//! transport's mechanics — not from a schematic: no netlist holds it, no
//! circuit method solves it, and its references are published formulas,
//! figures and measurements rather than ngspice. That is the `physical`
//! claim; the layer table names tape transport there already. The
//! hysteresis *device* stays in [`circuit::devices::magnetic`](crate::circuit::devices::magnetic)
//! (a transformer's core is a schematic part, and the same model), and
//! this module uses it. The record and playback *amplifiers* are
//! circuits: they belong in `circuit` when they are modelled.
//!
//! - [`Recording`] — record head and bias into the tape's
//!   magnetisation, at an oversampled rate.
//! - [`Playback`] — the play head's losses, an FIR from their formula,
//!   redesigned with the speed.
//! - [`Flutter`] — wow and flutter, a delay modulated by a measured
//!   transport's spectrum ([`Transport`]).
//! - [`Capstan`] — the tape's speed past the play head (DSP.md H5): slow
//!   [`Drift`], the [`Motor`]'s start and stop, [`Splices`]' jumps in
//!   time.
//! - [`Spacing`] — the head-to-tape spacing over time (H5): dropouts,
//!   splice joints and wear, a tape's [`Condition`], each a loss by the
//!   spacing law.
//! - [`Chowdhury`] — the three in a machine's order (record, transport,
//!   play) for one [`Machine`]: [`Machine::TC260`]; with a capstan and a
//!   tape condition after the play head when given (off by default).
//!
//! ```
//! use stave_dsp::physical::tape::{Chowdhury, Machine};
//! use stave_dsp::Stage;
//!
//! let mut deck = Chowdhury::new(Machine::TC260);
//! deck.prepare(48_000.0);
//! // A 1 kHz tone at -12 dBFS, recorded and played back.
//! let rms = |deck: &mut Chowdhury| {
//!     let mut sum = 0.0;
//!     for n in 0..48_000 {
//!         let y = deck.tick(0.25 * (std::f32::consts::TAU * 1e3 * n as f32 / 48e3).sin());
//!         if n >= 24_000 { sum += f64::from(y) * f64::from(y); }
//!     }
//!     (sum / 24_000.0).sqrt()
//! };
//! let fast = rms(&mut deck);
//! // At 3.75 ips the same tone loses more to the play head.
//! deck.set_speed(Machine::ips(3.75));
//! deck.reset();
//! let slow = rms(&mut deck);
//! assert!(slow < fast, "{slow} < {fast}");
//! ```

mod capstan;
mod flutter;
mod playback;
mod recording;
mod spacing;

#[cfg(test)]
mod tests;

pub use capstan::{Capstan, Drift, Motor, Splices, Stop};
pub use flutter::{Flutter, Transport};
pub use spacing::{Condition, Dropout, Spacing};
pub use playback::{Phase, Playback};
pub use recording::{Recording, BIAS_SAMPLES};

use crate::circuit::devices::magnetic::JilesAtherton;
use crate::kernel::node::{Stage, Tail};
use crate::kernel::oversampled::Oversampled;

/// A tape formulation: its magnetic material (Jiles–Atherton) and its
/// coating's thickness.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Formulation {
    pub magnetic: JilesAtherton,
    /// δ, metres.
    pub thickness: f64,
}

impl Formulation {
    /// Ferric oxide (γ-Fe₂O₃) tape, Chowdhury's (DAFx-19 §4.1, Fig. 6):
    /// Ms = 350 kA/m, coercivity (k) 27 kA/m, a = 22 kA/m, c = 0.17,
    /// α = 1.6e-3 (Jiles, Thoelke & Devine's method), 35 µm thick.
    pub const FERRIC: Self = Self {
        magnetic: JilesAtherton {
            saturation: 3.5e5,
            shape: 22e3,
            coupling: 1.6e-3,
            pinning: 27e3,
            reversibility: 0.17,
        },
        thickness: 35e-6,
    };
}

/// A record head: `turns` of wire, `efficiency`, and its `gap`
/// (metres).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RecordHead {
    pub turns: f64,
    pub efficiency: f64,
    pub gap: f64,
}

impl RecordHead {
    /// The deep-gap field per ampere of head current, `N·E/g` (A/m per A;
    /// Chowdhury eq. 16).
    pub fn field_per_amp(&self) -> f64 {
        self.turns * self.efficiency / self.gap
    }
}

/// A play head: its `gap` and its `spacing` from the tape (metres).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PlayHead {
    pub gap: f64,
    pub spacing: f64,
}

/// The bias: its frequency, and its current as a multiple of the
/// full-scale signal current (Chowdhury eq. 28).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bias {
    pub hz: f64,
    pub ratio: f64,
}

/// A tape machine's constants: heads, tape, speed, bias and the record
/// current at full scale.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Machine {
    pub record: RecordHead,
    pub play: PlayHead,
    pub formulation: Formulation,
    /// Tape speed, metres per second.
    pub speed: f64,
    pub bias: Bias,
    /// Record-head signal current at full-scale input (1.0), amperes.
    pub drive_amps: f64,
    /// Output per unit of `M / Ms`: the playback amplifier's gain.
    pub playback_gain: f64,
}

impl Machine {
    /// Inches per second as metres per second.
    pub const fn ips(inches_per_second: f64) -> f64 {
        inches_per_second * 0.0254
    }

    /// Sony TC-260 (Chowdhury, DAFx-19, §4 and Fig. 6): record head 100
    /// turns, efficiency 0.1, 6 µm gap; bias 55 kHz at five times the
    /// signal; ferric tape; 7.5 ips (it also runs at 3.75). The play head
    /// is Fig. 3's (5 µm gap, 20 µm spacing): the paper gives the TC-260's
    /// head width only.
    ///
    /// The paper's record current (0.1 mA peak to peak) makes 83 A/m
    /// through its own head constants, far below the tape's 27 kA/m
    /// coercivity, though it quotes a 5e5 A/m field; neither is used.
    /// Full scale here is the current that puts the bias's peak field
    /// at 2.5× the coercivity (bias 67.5 kA/m, signal 13.5 kA/m: 8.1 mA
    /// through these heads). At 1 kHz the record side then makes 0.13%
    /// third harmonic at full scale and 3% (a tape's maximum operating
    /// level) near +14 dBFS, about the headroom above a tape's 0 VU; the
    /// playback gain (7.72) plays −20 dBFS at 1 kHz back at −20 dBFS
    /// before the head losses.
    pub const TC260: Self = Self {
        record: RecordHead {
            turns: 100.0,
            efficiency: 0.1,
            gap: 6e-6,
        },
        play: PlayHead {
            gap: 5e-6,
            spacing: 20e-6,
        },
        formulation: Formulation::FERRIC,
        speed: Self::ips(7.5),
        bias: Bias {
            hz: 55e3,
            ratio: 5.0,
        },
        drive_amps: 13.5e3 / (100.0 * 0.1 / 6e-6),
        playback_gain: 7.72,
    };
}

/// The oversampled rate the record side runs at, about (the factor is
/// `round(INNER_RATE / rate)`: 705.6–768 kHz at the usual host rates).
const INNER_RATE: f64 = 720_000.0;

/// An analog tape machine's record-and-play path: the record head with
/// its bias into the tape's hysteresis (oversampled), the transport's
/// wow and flutter, and the play head's losses.
///
/// # Models
/// A reel-to-reel tape machine's tape path, after Chowdhury's physical
/// model of a Sony TC-260 (DAFx-19): record-head field and bias,
/// Jiles–Atherton tape magnetisation, wow and flutter, play-head
/// spacing, thickness and gap losses at the tape's speed. The machine's
/// record and playback amplifiers and their equalisation are not
/// modelled (the paper leaves them out too).
///
/// # Derivation
/// The paper's Fig. 7 signal flow: [`Recording`] at about 720 kHz (the
/// factor `round(720 kHz / rate)`, A2's linear-phase resampler, whose
/// decimating lowpass is the de-bias filter), the bias held at 14 of
/// those samples a cycle at every rate (50.4–54.9 kHz, the machine's
/// 55 moved by at most 8%: the bias's frequency matters to the tape
/// only by being far above the audio), then [`Flutter`] (a measured
/// transport, not the paper's unpublished TC-260 one), then
/// [`Playback`]; given, a [`Capstan`] (speed drift, start and stop,
/// splices) and a [`Spacing`] (dropouts, joints, wear) follow the play
/// head, in the tape's coordinates (DSP.md H5): off unless given, and
/// then bit-identical to the machine without them.
///
/// # Verified
/// `crates/stave-test/tests/tape_matches_reference.rs::loss_filter_matches_the_gap_loss_formula_at_every_speed`
/// (0.36 dB worst of eq. 13, budget 0.5),
/// `crates/stave-test/tests/tape_matches_reference.rs::loss_filter_matches_chowdhury_fig_3`
/// (0.28 dB of the figure's shape),
/// `crates/stave-test/tests/tape_matches_reference.rs::hysteresis_loop_against_chowdhury_fig_2`
/// (a known gap: the figure is not reproduced by the parameters it
/// names), `crates/stave-test/tests/tape_matches_reference.rs::harmonics_against_bias`,
/// `crates/stave-test/tests/tape_matches_reference.rs::aliasing_at_the_host_rate`
/// and `crates/stave-test/tests/tape_matches_reference.rs::the_same_tape_at_every_rate`
/// (44.1–192 kHz against 48 kHz: gain and third harmonic within 0.001 dB);
/// `crates/stave-test/tests/tape_matches_reference.rs::transport_off_is_bit_identical`
/// (an unconfigured capstan and condition change nothing).
///
/// # Cost
/// Datasheet `"Chowdhury tape (TC-260, 7.5 ips)"` (15× inside at 48 kHz),
/// and its parts `"Playback losses (7.5 ips)"` and `"Flutter (Akai 4000D)"`;
/// H5's `"Capstan (steady, studio motor, edited)"` and `"Spacing (used tape)"`
/// when given.
///
/// # Aliasing
/// The hysteresis runs at about 720 kHz, the bias on an even number of
/// those samples a cycle (so its harmonics fold onto themselves and the
/// loop stays half-wave symmetric), and A2's decimator rejects the rest:
/// tones to 2.5 kHz leave every inharmonic product of the record side
/// below −104 dB at −6 dBFS and −115 dB at +6 dBFS. Above, the bias
/// beats with the tone's harmonics (51.4 kHz − 4 × 10 kHz at 48 kHz), a
/// continuous-time product of the hysteresis: −96 dB at −6 dBFS, −61 dB
/// at +6 dBFS, for a 10 kHz tone.
///
/// # Valid ranges
/// Host rates 44.1–192 kHz; inputs to a few times full scale; speeds
/// 1.875–30 ips.
#[derive(Clone)]
pub struct Chowdhury {
    machine: Machine,
    record: Oversampled<Recording>,
    flutter: Flutter,
    playback: Playback,
    capstan: Option<Capstan>,
    spacing: Option<Spacing>,
    rate: f32,
}

impl Chowdhury {
    /// Prepared at 48 kHz.
    pub fn new(machine: Machine) -> Self {
        let mut recording = Recording::new(machine);
        recording.set_bias_samples(BIAS_SAMPLES);
        let mut c = Self {
            machine,
            record: Oversampled::linear(recording, 1),
            flutter: Flutter::new(Transport::AKAI_4000D),
            playback: Playback::new(machine.play, machine.formulation.thickness, machine.speed, Phase::Linear),
            capstan: None,
            spacing: None,
            rate: 0.0,
        };
        c.prepare(48_000.0);
        c
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// The record side's oversampling factor at the prepared rate.
    pub fn factor(&self) -> u32 {
        self.record.factor()
    }

    /// The transport's speed after the play head: drift, start and
    /// stop, splices (DSP.md H5). Prepares it at this machine's rate
    /// and speed; its latency joins the machine's.
    pub fn with_capstan(mut self, mut capstan: Capstan) -> Self {
        capstan.set_speed(self.machine.speed);
        capstan.prepare(self.rate);
        self.capstan = Some(capstan);
        self
    }

    /// The tape's condition: dropouts, splice joints, wear (DSP.md H5).
    /// Prepares it at this machine's rate and speed.
    pub fn with_spacing(mut self, mut spacing: Spacing) -> Self {
        spacing.set_speed(self.machine.speed);
        spacing.prepare(self.rate);
        self.spacing = Some(spacing);
        self
    }

    pub fn capstan(&mut self) -> Option<&mut Capstan> {
        self.capstan.as_mut()
    }

    pub fn spacing(&mut self) -> Option<&mut Spacing> {
        self.spacing.as_mut()
    }

    /// Tape speed, metres per second (see [`Machine::ips`]): redesigns
    /// the play head's losses (realtime).
    pub fn set_speed(&mut self, metres_per_second: f64) {
        self.machine.speed = metres_per_second;
        self.playback.set_speed(metres_per_second);
        if let Some(c) = self.capstan.as_mut() {
            c.set_speed(metres_per_second);
        }
        if let Some(s) = self.spacing.as_mut() {
            s.set_speed(metres_per_second);
        }
    }

    /// Bias current over the full-scale signal current (0 is unbiased:
    /// the dead zone).
    pub fn set_bias(&mut self, ratio: f64) {
        self.machine.bias.ratio = ratio;
        self.record.inner.set_bias(ratio);
    }

    /// Wow and flutter's depth: 1 the measured transport, 0 none.
    pub fn set_flutter(&mut self, depth: f64) {
        self.flutter.set_depth(depth);
    }

    pub fn recording(&self) -> &Recording {
        &self.record.inner
    }

    pub fn playback(&self) -> &Playback {
        &self.playback
    }
}

impl Stage for Chowdhury {
    fn prepare(&mut self, sample_rate: f32) {
        let factor = (INNER_RATE / f64::from(sample_rate)).round().max(1.0) as u32;
        if factor != self.record.factor() || sample_rate != self.rate {
            self.record = Oversampled::linear(self.record.inner.clone(), factor);
        }
        self.rate = sample_rate;
        self.record.prepare(sample_rate);
        self.flutter.prepare(sample_rate);
        self.playback.prepare(sample_rate);
        if let Some(c) = self.capstan.as_mut() {
            c.prepare(sample_rate);
        }
        if let Some(s) = self.spacing.as_mut() {
            s.prepare(sample_rate);
        }
    }
    fn reset(&mut self) {
        self.record.reset();
        self.flutter.reset();
        self.playback.reset();
        if let Some(c) = self.capstan.as_mut() {
            c.reset();
        }
        if let Some(s) = self.spacing.as_mut() {
            s.reset();
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let m = self.record.tick(x);
        let m = self.flutter.tick(m);
        let mut y = self.playback.tick(m);
        let mut ratio = 1.0;
        let mut spliced = false;
        if let Some(c) = self.capstan.as_mut() {
            y = c.tick(y);
            if c.is_enabled() {
                ratio = c.speed_ratio();
                spliced = c.spliced();
            }
        }
        if let Some(s) = self.spacing.as_mut() {
            if spliced {
                s.splice();
            }
            s.set_speed_ratio(ratio);
            y = s.tick(y);
        }
        crate::kernel::denormal::normal(y)
    }
    fn latency(&self) -> u32 {
        self.record.latency()
            + self.flutter.latency()
            + self.playback.latency()
            + self.capstan.as_ref().map_or(0, |c| c.latency())
            + self.spacing.as_ref().map_or(0, |s| s.latency())
    }
    fn tail(&self) -> Tail {
        Tail::Samples(2 * self.latency() + 4096)
    }
}
