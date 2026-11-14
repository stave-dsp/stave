//! Fuzzing the circuit solvers (DSP.md R7): a [`Case`] is one built unit
//! of a deck — its parts drawn inside their tolerances by E10's
//! [`Tolerances`], its controls at random positions — solved by
//! `Circuit<Dk>` or `Circuit<Wdf>` at a host rate, driven by a hot or odd
//! signal. [`run`] holds it to three things:
//!
//! - **finite**: every output sample is finite and within
//!   [`GARBAGE_VOLTS`] (no circuit here reaches a kilovolt);
//! - **reported**: each 64-sample block's [`Health`] (N8's account, what
//!   a plugin publishes through `SolverTaps`) is complete — a block that
//!   reports no cap hit ended every solve on a residual inside the
//!   solver's own convergence test, and a non-finite residual is a cap
//!   hit;
//! - **honest**: until either run reports a cap hit, the case solved
//!   again with a much harder Newton (eight times the iterations, eight
//!   times the continuation steps) agrees with it — a solve that said it
//!   converged did converge, to the same root.
//!
//! `fuzz/fuzz_targets/solvers.rs` decodes libFuzzer's bytes with
//! [`Case::from_bytes`]; `tests/solver_fuzz.rs` runs a seeded corpus
//! ([`Case::seeded`]) in CI and replays the committed regressions.

use std::fmt;

use stave_dsp::circuit::methods::adaa::Antialiasing;
use stave_dsp::circuit::methods::dk::{Dk, Health, Settings};
use stave_dsp::circuit::methods::newton::NewtonSettings;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::netlist::tolerance::{Tolerance, Tolerances};
use stave_dsp::circuit::netlist::{Netlist, Variable};
use stave_dsp::circuit::topologies::{self, diode_ladder::DiodeLadder, transistor_ladder::TransistorLadder};
use stave_dsp::circuit::Circuit;
use stave_dsp::primitives::random::splitmix64;
use stave_dsp::Stage;

/// A sample beyond this many volts is garbage, finite or not.
pub const GARBAGE_VOLTS: f64 = 1e5;

/// Samples a block: what a plugin publishes health for.
pub const BLOCK: usize = 64;

/// The rates a case runs at.
pub const RATES: [f32; 7] = [8_000.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0, 768_000.0];

/// A circuit the fuzzer builds: its netlist from up to three control
/// positions (0 to 1), the source it drives, and the input it works at
/// (a DC bias and a hot amplitude, volts).
pub struct Deck {
    pub name: &'static str,
    pub netlist: fn([f64; 3]) -> Netlist,
    pub source: &'static str,
    pub bias: f64,
    pub volts: f64,
}

fn parse(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap_or_else(|e| panic!("a committed deck does not parse: {e:?}"))
}

macro_rules! spice {
    ($file:literal) => {
        |_| parse(include_str!(concat!("../spice/", $file, ".cir")))
    };
}

/// Every deck `solver_taps.rs` drives hot (N8), and every classic
/// topology with its controls.
pub const DECKS: &[Deck] = &[
    Deck { name: "diode clipper", netlist: spice!("diode_clipper"), source: "v1", bias: 0.0, volts: 4.0 },
    Deck { name: "op-amp clipper", netlist: spice!("opamp_clipper"), source: "v1", bias: 0.0, volts: 1.0 },
    Deck {
        name: "common cathode",
        netlist: spice!("common_cathode_grid_current"),
        source: "v1",
        bias: 0.0,
        volts: 8.0,
    },
    Deck { name: "Tube Screamer", netlist: spice!("tube_screamer"), source: "v1", bias: 4.5, volts: 1.0 },
    Deck { name: "op-amp slew", netlist: spice!("opamp_slew"), source: "v1", bias: 0.0, volts: 3.0 },
    Deck { name: "OTA VCA", netlist: spice!("ota_vca"), source: "vin", bias: 0.0, volts: 20.0 },
    Deck { name: "EL34 push-pull", netlist: spice!("push_pull_pentode"), source: "vx", bias: 0.0, volts: 36.0 },
    Deck { name: "power supply", netlist: spice!("power_supply"), source: "il", bias: 0.05, volts: 0.04 },
    Deck { name: "fuzz face", netlist: spice!("fuzz_face"), source: "vin", bias: 0.0, volts: 2.0 },
    Deck { name: "rangemaster", netlist: spice!("rangemaster"), source: "vin", bias: 0.0, volts: 3.0 },
    Deck { name: "CE stage", netlist: spice!("ce_stage"), source: "vin", bias: 0.0, volts: 2.0 },
    Deck { name: "JFET booster", netlist: spice!("jfet_booster"), source: "vin", bias: 0.0, volts: 4.0 },
    Deck { name: "MOSFET booster", netlist: spice!("mosfet_booster"), source: "vin", bias: 0.0, volts: 8.0 },
    Deck { name: "Big Muff", netlist: |_| topologies::big_muff_rams_head(), source: "vin", bias: 0.0, volts: 1.0 },
    Deck { name: "TS-808", netlist: |_| topologies::tube_screamer_ts808(), source: "vin", bias: 0.0, volts: 1.0 },
    Deck { name: "ProCo RAT", netlist: |_| topologies::proco_rat(), source: "vin", bias: 0.0, volts: 1.0 },
    Deck { name: "Boss DS-1", netlist: |_| topologies::boss_ds1(), source: "vin", bias: 0.0, volts: 1.0 },
    Deck {
        name: "JCM800 2203",
        netlist: |_| topologies::marshall_jcm800_2203(),
        source: "vin",
        bias: 0.0,
        volts: 2.0,
    },
    Deck {
        name: "Klon Centaur",
        netlist: |c| topologies::klon_centaur(c[0], c[1]),
        source: "vin",
        bias: 0.0,
        volts: 1.0,
    },
    Deck {
        name: "Moog 904-A",
        netlist: |c| topologies::moog_ladder_904a(4.0 * c[0]),
        source: "vin",
        bias: 0.0,
        volts: 0.5,
    },
    Deck {
        name: "MXR Phase 90",
        netlist: |c| topologies::mxr_phase90(-2.0 + 6.0 * c[0]),
        source: "vin",
        bias: 0.0,
        volts: 1.0,
    },
    Deck { name: "CE-2 filters", netlist: |_| topologies::boss_ce2_filters(), source: "vin", bias: 0.0, volts: 1.0 },
    Deck {
        name: "Big Muff tone",
        netlist: |c| topologies::big_muff_tone_stack(c[0]),
        source: "vin",
        bias: 0.0,
        volts: 2.0,
    },
    Deck {
        name: "AB763 tone stack",
        netlist: |c| topologies::fender_ab763_tone_stack(c[0], c[1]),
        source: "vin",
        bias: 0.0,
        volts: 2.0,
    },
    Deck {
        name: "transistor ladder",
        netlist: |c| TransistorLadder { resonance: 5.0 * c[0], ..TransistorLadder::DEFAULT }.netlist(),
        source: "vin",
        bias: 0.0,
        volts: 0.1,
    },
    Deck {
        name: "diode ladder",
        netlist: |c| DiodeLadder { resonance: 12.0 * c[0], ..DiodeLadder::DEFAULT }.netlist(),
        source: "vin",
        bias: 0.0,
        volts: 0.1,
    },
];

/// The solver a case runs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Method {
    Dk,
    /// `Circuit<Dk>` with first-order stateful ADAA.
    DkAdaa,
    Wdf,
}

/// What drives the circuit, about its bias, in units of the deck's hot
/// amplitude.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Stimulus {
    /// A sine at `hz` (folded below Nyquist).
    Sine { hz: f64 },
    /// A naive square at `hz`: every edge a step.
    Square { hz: f64 },
    /// Alternating ±1 every sample: the Nyquist tone.
    Nyquist,
    /// One sample of `amplitude`, then the bias.
    Impulse,
    /// A step to `amplitude` held.
    Step,
    /// Seeded uniform noise.
    Noise { seed: u64 },
}

/// One fuzz case: a built unit of a deck, its solver, rate and drive.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Case {
    pub deck: usize,
    pub method: Method,
    /// The tolerance batch and unit (E10's seed and unit number).
    pub seed: u64,
    pub unit: u64,
    /// Grades: resistors, capacitors (and inductors), pots — fractions
    /// in E10's valid range, `0 ≤ t < 1`.
    pub grades: [f64; 3],
    /// Control positions, 0 to 1: the topology's arguments, then any
    /// pot or rheostat the netlist declares.
    pub controls: [f64; 3],
    pub rate: f32,
    pub stimulus: Stimulus,
    /// The drive as a multiple of the deck's hot amplitude.
    pub level: f64,
    /// A DC offset on top of the deck's bias, in hot amplitudes.
    pub offset: f64,
    pub samples: usize,
}

/// Bytes read in order, zeros once they run out: any input is a case.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        match self.0.split_first() {
            Some((&b, rest)) => {
                self.0 = rest;
                b
            }
            None => 0,
        }
    }
    fn u64(&mut self) -> u64 {
        (0..8).fold(0u64, |a, _| (a << 8) | u64::from(self.u8()))
    }
    /// 0 to 1 inclusive.
    fn unit(&mut self) -> f64 {
        f64::from(self.u8()) / 255.0
    }
}

impl Case {
    /// The case `data` spells: libFuzzer's bytes, read as fields in
    /// order (deck, method, rate, stimulus, level, offset, grades,
    /// controls, seed, unit); missing bytes are zero.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut b = Bytes(data);
        let deck = usize::from(b.u8()) % DECKS.len();
        let method = [Method::Dk, Method::DkAdaa, Method::Wdf][usize::from(b.u8()) % 3];
        let rate = RATES[usize::from(b.u8()) % RATES.len()];
        let kind = b.u8();
        // 10 Hz to 40 kHz, logarithmically.
        let hz = 10.0 * 4_000f64.powf(b.unit());
        let stimulus = match kind % 6 {
            0 => Stimulus::Sine { hz },
            1 => Stimulus::Square { hz },
            2 => Stimulus::Nyquist,
            3 => Stimulus::Impulse,
            4 => Stimulus::Step,
            _ => Stimulus::Noise { seed: u64::from(kind) },
        };
        // A tenth to ten times the deck's hot level, logarithmically.
        let level = 10f64.powf(2.0 * b.unit() - 1.0);
        let offset = 2.0 * b.unit() - 1.0;
        // Up to ±50 % on every part: far outside any grade sold, inside
        // E10's valid range.
        let grades = [0.5 * b.unit(), 0.5 * b.unit(), 0.3 * b.unit()];
        let controls = [b.unit(), b.unit(), b.unit()];
        let (seed, unit) = (b.u64(), u64::from(b.u8()));
        // Short: building and preparing the unit is most of a case, and
        // an onset's transient most of what can break.
        Self { deck, method, seed, unit, grades, controls, rate, stimulus, level, offset, samples: 256 }
    }

    /// Case `n` of the seeded corpus: its bytes drawn from SplitMix64.
    pub fn seeded(n: u64) -> Self {
        let mut state = splitmix64(0x5EED_0000 ^ n);
        let bytes: Vec<u8> = (0..32)
            .flat_map(|_| {
                state = splitmix64(state);
                state.to_le_bytes()
            })
            .take(32)
            .collect();
        // Longer than a fuzzed case: CI runs few, so each runs on.
        Self { samples: 2_048, ..Self::from_bytes(&bytes) }
    }

    /// The netlist this unit is built to.
    pub fn netlist(&self) -> Result<Netlist, String> {
        let deck = &DECKS[self.deck];
        let base = (deck.netlist)(self.controls);
        let grades = Tolerances::new()
            .resistors(Tolerance::uniform(self.grades[0]))
            .capacitors(Tolerance::uniform(self.grades[1]))
            .inductors(Tolerance::uniform(self.grades[1]))
            .pots(Tolerance::uniform(self.grades[2]));
        grades.unit(&base, self.seed, self.unit).map_err(|e| format!("{e:?}"))
    }

    /// Input sample `n`, volts.
    pub fn input(&self, n: usize) -> f64 {
        let deck = &DECKS[self.deck];
        let a = deck.volts * self.level;
        let nyquist = f64::from(self.rate) / 2.0;
        let phase = |hz: f64| {
            let hz = hz.min(0.98 * nyquist);
            (hz * n as f64 / f64::from(self.rate)).fract()
        };
        let x = match self.stimulus {
            Stimulus::Sine { hz } => (std::f64::consts::TAU * phase(hz)).sin(),
            Stimulus::Square { hz } => {
                if phase(hz) < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Stimulus::Nyquist => {
                if n % 2 == 0 {
                    1.0
                } else {
                    -1.0
                }
            }
            Stimulus::Impulse => f64::from(u8::from(n == 16)),
            Stimulus::Step => f64::from(u8::from(n >= 16)),
            Stimulus::Noise { seed } => {
                (splitmix64(seed ^ splitmix64(n as u64)) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
            }
        };
        deck.bias + a * (self.offset + x)
    }
}

/// What a case did: its solves, and what the health taps showed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The method refused the netlist (a WDF cannot adapt every
    /// circuit): nothing ran.
    pub refused: Option<String>,
    pub solves: u64,
    /// Cap hits the taps reported (each a sample whose solve ended
    /// unconverged, said so).
    pub capped: u64,
    pub continuations: u64,
    /// The largest output, volts.
    pub peak: f64,
    /// Samples the harder solve was compared over (until a cap hit).
    pub compared: usize,
    /// The largest difference from the harder solve over them.
    pub divergence: f64,
}

/// A case that broke a rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub case: Case,
    pub rule: &'static str,
    pub detail: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {} — {:?}", DECKS[self.case.deck].name, self.rule, self.detail, self.case)
    }
}

/// A circuit under either method, health read per block.
enum Solver {
    Dk(Box<Circuit<Dk>>),
    Wdf(Box<Circuit<Wdf>>),
}

impl Solver {
    fn build(case: &Case, netlist: &Netlist, settings: Settings) -> Result<Self, String> {
        let deck = &DECKS[case.deck];
        let mut s = match case.method {
            Method::Dk | Method::DkAdaa => {
                let mut c = Circuit::<Dk>::new(netlist).map_err(|e| format!("{e:?}"))?;
                c.set_settings(settings);
                c.drive(deck.source).map_err(|e| format!("{e:?}"))?;
                if case.method == Method::DkAdaa {
                    c.set_antialiasing(Antialiasing::Adaa);
                }
                Self::Dk(Box::new(c))
            }
            Method::Wdf => {
                let mut c = Circuit::<Wdf>::new(netlist).map_err(|e| format!("{e:?}"))?;
                c.set_settings(settings);
                c.drive(deck.source).map_err(|e| format!("{e:?}"))?;
                Self::Wdf(Box::new(c))
            }
        };
        // Declared pots and rheostats at the case's positions, the
        // first one's after the topology's own controls.
        for (i, v) in netlist.variables().iter().enumerate() {
            if matches!(v, Variable::Pot { .. } | Variable::Rheostat { .. }) {
                let position = case.controls[(i + 1) % 3];
                match &mut s {
                    Self::Dk(c) => {
                        let pot = c.pot(v.name()).map_err(|e| format!("{e:?}"))?;
                        c.set_position(pot, position);
                    }
                    Self::Wdf(c) => {
                        let pot = c.pot(v.name()).map_err(|e| format!("{e:?}"))?;
                        c.set_position(pot, position);
                    }
                }
            }
        }
        match &mut s {
            Self::Dk(c) => c.prepare(case.rate),
            Self::Wdf(c) => c.prepare(case.rate),
        }
        Ok(s)
    }

    fn tick(&mut self, x: f64) -> f64 {
        match self {
            Self::Dk(c) => c.tick_f64(x),
            Self::Wdf(c) => c.tick_f64(x),
        }
    }

    fn take_health(&mut self) -> Health {
        match self {
            Self::Dk(c) => c.take_health(),
            Self::Wdf(c) => c.take_health(),
        }
    }
}

/// Settings far harder than the default: eight times the iterations and
/// the continuation steps, a finer damping floor.
pub const HARD: Settings = Settings {
    newton: NewtonSettings {
        max_iterations: 8 * NewtonSettings::DEFAULT.max_iterations,
        min_damping: NewtonSettings::DEFAULT.min_damping / 16.0,
        ..NewtonSettings::DEFAULT
    },
    continuation_steps: 8 * Settings::DEFAULT.continuation_steps,
};

/// The residual a block reporting no cap hit may show: the solver's own
/// convergence test (a microvolt per volt of `1 + max |v|`) at the
/// largest `v` the case could reach.
fn converged_residual() -> f64 {
    1e-6 * (1.0 + GARBAGE_VOLTS)
}

/// How far the harder solve may sit from a solve that converged: well
/// above Newton's tolerance carried through the state, far below any
/// wrong root.
fn agreement(peak: f64) -> f64 {
    1e-4 * (1.0 + peak)
}

/// Run `case`: [`Report`] when it keeps every rule, else the first
/// [`Failure`].
pub fn run(case: &Case) -> Result<Report, Box<Failure>> {
    let fail = |rule, detail: String| Box::new(Failure { case: *case, rule, detail });
    let netlist = case.netlist().map_err(|e| fail("builds", format!("the unit does not build: {e}")))?;
    let mut solver = match Solver::build(case, &netlist, Settings::DEFAULT) {
        Ok(s) => s,
        // A method may refuse a netlist; the solver itself built it in
        // the deck's nominal values, so a drawn unit it refuses is a
        // refusal too, not a failure.
        Err(e) => return Ok(Report { refused: Some(e), ..Report::default() }),
    };
    let mut hard = Solver::build(case, &netlist, HARD).ok();
    let mut report = Report::default();
    let mut comparing = hard.is_some();
    let mut out = vec![0.0f64; BLOCK];
    for start in (0..case.samples).step_by(BLOCK) {
        let len = BLOCK.min(case.samples - start);
        for (i, y) in out[..len].iter_mut().enumerate() {
            let n = start + i;
            *y = solver.tick(case.input(n));
            if !y.is_finite() || y.abs() > GARBAGE_VOLTS {
                return Err(fail("finite", format!("sample {n} is {y:e} V")));
            }
            report.peak = report.peak.max(y.abs());
        }
        let health = solver.take_health();
        report.solves += health.newton.solves;
        report.capped += health.newton.capped;
        report.continuations += health.continuations;
        // A NaN residual is a cap hit too.
        let residual = health.max_residual;
        if health.newton.capped == 0 && (residual.is_nan() || residual > converged_residual()) {
            return Err(fail(
                "reported",
                format!(
                    "block at {start}: no cap hit reported, but a solve ended on a residual of {:e} V",
                    health.max_residual
                ),
            ));
        }
        if let (true, Some(h)) = (comparing, hard.as_mut()) {
            let mut worst = 0.0f64;
            for (i, y) in out[..len].iter().enumerate() {
                worst = worst.max((h.tick(case.input(start + i)) - y).abs());
            }
            if h.take_health().newton.capped > 0 || health.newton.capped > 0 {
                comparing = false;
            } else {
                report.compared += len;
                report.divergence = report.divergence.max(worst);
                if worst > agreement(report.peak) {
                    return Err(fail(
                        "honest",
                        format!(
                            "block at {start}: converged, yet {worst:e} V from the harder solve (peak {:.3} V)",
                            report.peak
                        ),
                    ));
                }
            }
        }
    }
    Ok(report)
}
