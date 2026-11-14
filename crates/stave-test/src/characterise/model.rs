//! The model side: a circuit (`Circuit<Dk>` on the deck) or a hand
//! topology (any ticking stage), run through a plan at one rate into a
//! [`Table`] of the same measurements the reference holds.

use stave_dsp::circuit::methods::adaa::Antialiasing;
use stave_dsp::circuit::methods::dk::{Dk, Phasor};
use stave_dsp::circuit::methods::newton::NewtonStats;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;
use stave_dsp::kernel::node::Tail;
use stave_measure::suite::analysis::{
    aliasing, anmr, bins, gain, steady_state, thd_n, waveform, Harmonics, Spectrum,
};
use stave_measure::suite::excitation::Excitation;
use stave_measure::suite::levels::{find_knee, Knee};
use stave_measure::suite::period::{harmonic_bins, tone_bin, Period, SHIPPING_RATES};
use stave_measure::suite::{db, Complex};

use super::compare::averaged_bla;
use super::Shipping;
use super::plan::{Input, Kind, Plan, Run, Tier, HARMONICS};
use super::table::{Measured, Table};

/// The name of the driven source a plan's input is split into.
pub const DRIVEN: &str = "char";

/// The deck with its input split: the input element keeps its DC level
/// and a driven source (`vchar` in series, or `ichar` in parallel)
/// carries the stimulus; held sources are pinned to DC.
pub fn split(deck: &Netlist, input: &Input) -> (Netlist, String) {
    let mut n = deck.clone();
    for (source, dc) in &input.holds {
        let element = n
            .element_mut(source)
            .unwrap_or_else(|| panic!("no source {source} to hold"));
        match &mut element.kind {
            ElementKind::VoltageSource { source, .. } | ElementKind::CurrentSource { source, .. } => {
                *source = Source::dc(*dc);
            }
            other => panic!("{other:?} is not a source"),
        }
    }
    let element = n
        .remove(&input.source)
        .unwrap_or_else(|| panic!("no input source {}", input.source));
    let driven = |dc: f64| Source {
        dc,
        ac: 1.0,
        signal: None,
    };
    let name = match element.kind {
        ElementKind::VoltageSource { a, b, .. } => {
            let x = n.add_node("char_x");
            let kind = ElementKind::VoltageSource {
                a,
                b: x,
                source: Source::dc(input.dc),
            };
            n.add(&element.name, kind).unwrap();
            let kind = ElementKind::VoltageSource {
                a: x,
                b,
                source: driven(0.0),
            };
            n.add(&format!("v{DRIVEN}"), kind).unwrap();
            format!("v{DRIVEN}")
        }
        ElementKind::CurrentSource { a, b, .. } => {
            let kind = ElementKind::CurrentSource {
                a,
                b,
                source: Source::dc(input.dc),
            };
            n.add(&element.name, kind).unwrap();
            let kind = ElementKind::CurrentSource {
                a,
                b,
                source: driven(0.0),
            };
            n.add(&format!("i{DRIVEN}"), kind).unwrap();
            format!("i{DRIVEN}")
        }
        other => panic!("input {} is not a source: {other:?}", input.source),
    };
    (n, name)
}

/// `Circuit<Dk>` on the split deck, driven, at `rate`.
pub fn circuit(deck: &Netlist, input: &Input, rate: f64) -> Circuit<Dk> {
    <Circuit<Dk> as Solver>::on(deck, input, rate)
}

/// `Circuit<Wdf>` on the split deck, driven, at `rate` (its devices
/// solved statically: compare it with DK on `deck.without_charge()`).
pub fn wdf_circuit(deck: &Netlist, input: &Input, rate: f64) -> Circuit<Wdf> {
    <Circuit<Wdf> as Solver>::on(deck, input, rate)
}

/// A circuit method a plan runs a netlist through.
trait Solver: Clone {
    fn on(deck: &Netlist, input: &Input, rate: f64) -> Self;
    fn output(&self) -> f64;
    fn tick(&mut self, x: f64) -> f64;
    fn stats(&self) -> NewtonStats;
    /// The small-signal response at `hz`: analog (prewarped) or discrete.
    fn response(&self, hz: f64, analog: bool) -> Option<Phasor>;
    /// Prewarp every reactance to `hz` (R12).
    fn set_prewarp(&mut self, hz: Option<f64>);
    /// Stateful ADAA in the solve (A1).
    fn set_antialiasing(&mut self, a: Antialiasing);
}

impl Solver for Circuit<Dk> {
    fn on(deck: &Netlist, input: &Input, rate: f64) -> Self {
        let (netlist, driven) = split(deck, input);
        let mut c = Circuit::<Dk>::new(&netlist).unwrap_or_else(|e| panic!("{e:?}"));
        c.drive(&driven).unwrap();
        c.prepare(rate as f32);
        c
    }
    fn output(&self) -> f64 {
        Dk::output(self)
    }
    fn tick(&mut self, x: f64) -> f64 {
        self.tick_f64(x)
    }
    fn stats(&self) -> NewtonStats {
        Dk::stats(self)
    }
    fn set_prewarp(&mut self, hz: Option<f64>) {
        Dk::set_prewarp(self, hz);
    }
    fn set_antialiasing(&mut self, a: Antialiasing) {
        Dk::set_antialiasing(self, a);
    }
    fn response(&self, hz: f64, analog: bool) -> Option<Phasor> {
        if analog {
            self.analog_response(hz)
        } else {
            self.frequency_response(hz)
        }
    }
}

impl Solver for Circuit<Wdf> {
    fn on(deck: &Netlist, input: &Input, rate: f64) -> Self {
        let (netlist, driven) = split(deck, input);
        let mut c = Circuit::<Wdf>::new(&netlist).unwrap_or_else(|e| panic!("{e:?}"));
        c.drive(&driven).unwrap();
        c.prepare(rate as f32);
        c
    }
    fn output(&self) -> f64 {
        Wdf::output(self)
    }
    fn tick(&mut self, x: f64) -> f64 {
        self.tick_f64(x)
    }
    fn stats(&self) -> NewtonStats {
        Wdf::stats(self)
    }
    fn set_prewarp(&mut self, hz: Option<f64>) {
        Wdf::set_prewarp(self, hz);
    }
    fn set_antialiasing(&mut self, a: Antialiasing) {
        Wdf::set_antialiasing(self, a);
    }
    fn response(&self, hz: f64, analog: bool) -> Option<Phasor> {
        if analog {
            self.analog_response(hz)
        } else {
            self.frequency_response(hz)
        }
    }
}

/// A fresh ticking model: the stimulus at `i / rate` in, the output then out.
pub type Ticker = Box<dyn FnMut(f64) -> f64>;

/// What a plan is run on.
pub enum Model {
    /// `Circuit<Dk>` on a netlist (the deck, or the deck with stave's
    /// devices in), split by the plan's input.
    Circuit(Netlist),
    /// `Circuit<Wdf>` on a netlist, split the same way; its operating
    /// point is the DK method's DC analysis of the same netlist.
    Wdf(Netlist),
    /// A hand topology: `make(rate)` returns a fresh ticker, called with
    /// the stimulus at `i / rate` for `i = 0, 1, …` and returning the
    /// output then; `operating_point` lists the node voltages it knows.
    Stage {
        make: Box<dyn Fn(f64) -> Ticker>,
        operating_point: Vec<(String, f64)>,
    },
    /// A black box that answers late (a plugin with latency, a block
    /// renderer): `make(rate)` returns a fresh ticker and its delay in
    /// samples; each run is fed that many samples further so the output
    /// lines up with its stimulus ([`super::black_box`]).
    Delayed { make: Box<Maker> },
}

/// A fresh delayed ticker at a rate, and its delay in samples.
pub type Maker = dyn Fn(f64) -> (Ticker, usize);

/// One run at `rate`: the measured periods and the one before them.
fn simulate(run: &Run, rate: f64, tick: &mut dyn FnMut(f64) -> f64) -> Vec<Vec<f64>> {
    simulate_delayed(run, rate, tick, 0)
}

/// [`simulate`] for a ticker whose output is `delay` samples late: the
/// stimulus runs `delay` samples longer and the first `delay` outputs
/// are dropped, so sample `i` out answers sample `i` in.
fn simulate_delayed(run: &Run, rate: f64, tick: &mut dyn FnMut(f64) -> f64, delay: usize) -> Vec<Vec<f64>> {
    let n = run.period().samples(rate);
    let total = (run.settle + run.measure) as usize * n;
    let keep = (run.settle as usize - 1) * n + delay;
    let mut out = Vec::with_capacity(total + delay - keep);
    for i in 0..total + delay {
        let y = tick(run.excitation.at(i as f64 / rate));
        if i >= keep {
            out.push(y);
        }
    }
    out.chunks(n).map(<[f64]>::to_vec).collect()
}

/// The highest rate the ANMR is read at: a host rate, where aliasing is
/// what a listener hears (an oversampled solver's figure says nothing
/// about the plugin).
pub const ANMR_MAX_RATE: f64 = 96_000.0;

/// A run's measurements from its simulated periods (the first is the
/// one before the measured ones).
pub fn measure_run(run: &Run, rate: f64, periods: &[Vec<f64>]) -> Measured {
    measure_decimated(run, rate, periods, 1)
}

/// The same, for a model run at `m` times its host rate (`rate` is the
/// run's): every period read through an ideal decimator — the bins below
/// the host's Nyquist kept, the rest gone — as a plugin oversampling by
/// `m` hands them to the host.
pub fn measure_decimated(run: &Run, rate: f64, periods: &[Vec<f64>], m: u32) -> Measured {
    let p = run.period();
    let host = rate / f64::from(m.max(1));
    let nh = p.samples(host) as u64;
    let below = |k: &u32| 2 * u64::from(*k) < nh;
    let spectra: Vec<Spectrum> = periods[1..]
        .iter()
        .map(|x| {
            let wanted: Vec<u32> = run.bins.iter().copied().filter(below).collect();
            bins(x, p, rate, &wanted)
        })
        .collect();
    let decimated;
    let (rate, last) = if m > 1 {
        let all: Vec<u32> = (0..(p.samples(host) as u32).div_ceil(2)).collect();
        decimated = waveform(&bins(&periods[1], p, rate, &all), p, host);
        (host, &decimated)
    } else {
        (rate, &periods[1])
    };
    let (thdn, floor, anmr) = match run.kind {
        Kind::Tone { bin, .. } => {
            (
                Some(thd_n(last, p, rate, bin)),
                Some(aliasing(last, p, rate, bin)),
                (rate <= ANMR_MAX_RATE).then(|| anmr(last, p, rate, bin)),
            )
        }
        _ => (None, None, None),
    };
    Measured {
        steady: steady_state(&periods[0], &periods[1]),
        thdn,
        floor,
        anmr,
        periods: spectra,
    }
}

/// Solver health over a measurement: Newton solves that hit their cap.
#[derive(Copy, Clone, Debug, Default)]
pub struct Health {
    pub capped: u64,
    pub solves: u64,
}

impl Model {
    /// Run `plan`'s `tier` at `rate` into a table; `nodes` are the
    /// operating-point nodes to read (the reference's). A circuit is
    /// prewarped and anti-aliased as `config` says (R12; its oversampling
    /// is already in `rate`); a hand topology has its own discretisation
    /// and ignores it.
    #[allow(clippy::too_many_arguments)]
    pub fn measure(
        &self,
        plan: &Plan,
        deck: &Netlist,
        tier: Tier,
        rate: f64,
        oversampled: bool,
        config: Shipping,
        nodes: &[String],
    ) -> (Table, Health) {
        let mut table = Table {
            circuit: plan.circuit.clone(),
            source: format!("the model at {rate} Hz"),
            ..Table::default()
        };
        let mut health = Health::default();
        match self {
            Model::Circuit(netlist) => {
                measure_circuit::<Circuit<Dk>>(netlist, plan, tier, rate, oversampled, config, nodes, &mut table, &mut health);
            }
            Model::Wdf(netlist) => {
                measure_circuit::<Circuit<Wdf>>(netlist, plan, tier, rate, oversampled, config, nodes, &mut table, &mut health);
            }
            Model::Stage {
                make,
                operating_point,
            } => {
                table.op = operating_point
                    .iter()
                    .filter(|(n, _)| nodes.contains(n))
                    .cloned()
                    .collect();
                for run in plan.runs(tier) {
                    let mut tick = make(rate);
                    let periods = simulate(run, rate, &mut *tick);
                    table.runs.insert(run.id.clone(), measure_decimated(run, rate, &periods, config.oversample));
                }
                black_box_response(plan, tier, &mut table);
            }
            Model::Delayed { make } => {
                for run in plan.runs(tier) {
                    let (mut tick, delay) = make(rate);
                    let periods = simulate_delayed(run, rate, &mut *tick, delay);
                    table.runs.insert(run.id.clone(), measure_run(run, rate, &periods));
                }
                black_box_response(plan, tier, &mut table);
            }
        }
        let _ = deck;
        (table, health)
    }
}

impl Model {
    /// The model's response at `rate` to `input`, sample for sample
    /// (output `i` answers input `i`): a circuit from its operating point
    /// with `plan`'s input split, a stage or black box fresh from `make`,
    /// a delayed one fed its delay further. For stimuli that are not a
    /// plan's runs (C12's sweeps, [`super::kernels`]).
    pub fn respond(&self, plan: &Plan, rate: f64, input: &[f64]) -> Vec<f64> {
        fn through<S: Solver>(netlist: &Netlist, plan: &Plan, rate: f64, input: &[f64]) -> Vec<f64> {
            let mut c = S::on(netlist, &plan.input, rate);
            input.iter().map(|&x| c.tick(x)).collect()
        }
        match self {
            Model::Circuit(netlist) => through::<Circuit<Dk>>(netlist, plan, rate, input),
            Model::Wdf(netlist) => through::<Circuit<Wdf>>(netlist, plan, rate, input),
            Model::Stage { make, .. } => {
                let mut tick = make(rate);
                input.iter().map(|&x| tick(x)).collect()
            }
            Model::Delayed { make } => {
                let (tick, delay) = make(rate);
                let fed = input.iter().copied().chain(std::iter::repeat_n(0.0, delay));
                fed.map(tick).skip(delay).collect()
            }
        }
    }
}

/// C2 of a black box: the −40 dB multisine's BLA (averaged over its
/// realisations) at the response points.
fn black_box_response(plan: &Plan, tier: Tier, table: &mut Table) {
    if let Some(bla) = averaged_bla(plan, tier, table, -40.0) {
        table.ac = bla
            .into_iter()
            .map(|(k, g)| (super::plan::MULTISINE_PERIOD.hz(k), g))
            .collect();
    }
}

/// `netlist` through `plan` on the method `S`: the operating point (the
/// DK method's DC analysis, which every method starts from), the
/// small-signal response, and every run.
#[allow(clippy::too_many_arguments)]
fn measure_circuit<S: Solver>(
    netlist: &Netlist,
    plan: &Plan,
    tier: Tier,
    rate: f64,
    oversampled: bool,
    config: Shipping,
    nodes: &[String],
    table: &mut Table,
    health: &mut Health,
) {
    let mut base = S::on(netlist, &plan.input, rate);
    if config.prewarp.is_some() {
        base.set_prewarp(config.prewarp);
    }
    if config.adaa {
        base.set_antialiasing(Antialiasing::Adaa);
    }
    let uic = netlist.tran.is_some_and(|t| t.uic);
    if !uic {
        let dk = circuit(netlist, &plan.input, rate);
        let (split_netlist, _) = split(netlist, &plan.input);
        for node in nodes {
            if let Some(n) = split_netlist.node(node) {
                let mut c = dk.clone();
                c.set_probe(Probe::node(n));
                table.op.push((node.clone(), c.output()));
            }
        }
    }
    // ADAA's half inner sample, reported as latency (`Shipping::latency_seconds`):
    // every phase is read with it taken out.
    let latency = config.latency_seconds(rate / f64::from(config.oversample.max(1)));
    let advance = |hz: f64, c: Complex<f64>| {
        if latency == 0.0 {
            return c;
        }
        let (s, co) = (std::f64::consts::TAU * hz * latency).sin_cos();
        Complex::new(c.re * co - c.im * s, c.re * s + c.im * co)
    };
    for &hz in &plan.response_hz {
        if hz < rate / 2.0 {
            if let Some(r) = base.response(hz, oversampled) {
                table.ac.push((hz, advance(hz, Complex::new(r.re, r.im))));
            }
        }
    }
    for run in plan.runs(tier) {
        let mut c = base.clone();
        let mut first = true;
        let mut tick = |x: f64| {
            if std::mem::take(&mut first) {
                c.output()
            } else {
                c.tick(x)
            }
        };
        let periods = simulate(run, rate, &mut tick);
        let stats = c.stats();
        health.capped += stats.capped;
        health.solves += stats.solves;
        let mut measured = measure_decimated(run, rate, &periods, config.oversample);
        if latency != 0.0 {
            let hz = 1.0 / run.period().seconds();
            for spectrum in &mut measured.periods {
                for (k, c) in &mut spectrum.bins {
                    *c = advance(f64::from(*k) * hz, *c);
                }
            }
        }
        table.runs.insert(run.id.clone(), measured);
    }
}

/// The settling time from the linearised circuit's slowest pole: half
/// its 120 dB tail (60 dB), between 1 ms and 50 ms, to 0.1 ms; none for
/// a memoryless circuit.
pub fn settle_seconds(deck: &Netlist, input: &Input) -> f64 {
    let c = circuit(deck, input, 48_000.0);
    let seconds = match c.tail() {
        Tail::None => return 0.0,
        Tail::Samples(n) => f64::from(n) / 48_000.0 / 2.0,
        Tail::Infinite => 0.05,
    };
    (seconds.clamp(1e-3, 0.05) * 1e4).round() / 1e4
}

/// The circuit's knee at 1 kHz (DSP.md principle 3): `Circuit<Dk>` at
/// 4× 48 kHz, a sine climbing from 1 µV to 100 V until the gain moves
/// 1 dB or THD (H2–H15) reaches 1 %, rounded to 0.1 dB.
pub fn knee(deck: &Netlist, input: &Input, settle: f64) -> Knee {
    let rate = 192_000.0;
    let base = circuit(deck, input, rate);
    let period = Period::units(6);
    let bin = tone_bin(period, 1_000.0, HARMONICS, &SHIPPING_RATES).unwrap();
    let settle_periods = (settle / period.seconds()).ceil() as u32 + 1;
    let mut wanted = vec![0];
    wanted.extend(harmonic_bins(bin, HARMONICS).iter().map(|&b| b as u32));
    let measure = |amplitude: f64| {
        let e = Excitation::sine(period, bin, amplitude)
            .faded_in(0.5 * settle.max(1e-3));
        let run = Run {
            id: "knee".into(),
            tier: Tier::T0,
            kind: Kind::Tone {
                bin,
                offset_db: 0.0,
            },
            settle: settle_periods,
            measure: 1,
            bins: wanted.clone(),
            excitation: e,
        };
        let mut c = base.clone();
        let mut first = true;
        let mut tick = |x: f64| {
            if std::mem::take(&mut first) {
                c.output()
            } else {
                c.tick_f64(x)
            }
        };
        let periods = simulate(&run, rate, &mut tick);
        let s = bins(&periods[1], period, rate, &wanted);
        let h = Harmonics::of(&s, bin, HARMONICS);
        (db(gain(&run.excitation.tones[0], h.fundamental()).norm()), h.thd())
    };
    find_knee(measure, 1e-6, 100.0).rounded()
}

/// A black box's knee at 1 kHz (DSP.md principle 3), as [`knee`] finds a
/// circuit's: a fresh ticker from `make` at `rate` for each level, a
/// sine climbing from 10 µ to 100 (of full scale) until the gain moves
/// 1 dB or THD reaches 1 %, rounded to 0.1 dB.
pub fn stage_knee(make: &dyn Fn(f64) -> Ticker, rate: f64, settle: f64) -> Knee {
    delayed_knee(&|r| (make(r), 0), rate, settle)
}

/// [`stage_knee`] for a delayed black box ([`Model::Delayed`]).
pub fn delayed_knee(make: &dyn Fn(f64) -> (Ticker, usize), rate: f64, settle: f64) -> Knee {
    delayed_knee_from(make, rate, settle, KNEE_SEARCH_FROM)
}

/// Where [`stage_knee`]'s climb starts: 10 µ of full scale.
pub const KNEE_SEARCH_FROM: f64 = 1e-5;

/// [`delayed_knee`] with the climb starting at `from` (of full scale):
/// for a black box whose small-signal reading is its own `f32` floor (a
/// network whose output sits on a DC offset reads a tiny tone's
/// harmonics as that offset's rounding: a "knee" at the first level).
pub fn delayed_knee_from(make: &dyn Fn(f64) -> (Ticker, usize), rate: f64, settle: f64, from: f64) -> Knee {
    let period = Period::units(6);
    let bin = tone_bin(period, 1_000.0, HARMONICS, &SHIPPING_RATES).unwrap();
    let settle_periods = (settle / period.seconds()).ceil() as u32 + 1;
    let mut wanted = vec![0];
    wanted.extend(harmonic_bins(bin, HARMONICS).iter().map(|&b| b as u32));
    let measure = |amplitude: f64| {
        let run = Run {
            id: "knee".into(),
            tier: Tier::T0,
            kind: Kind::Tone {
                bin,
                offset_db: 0.0,
            },
            settle: settle_periods,
            measure: 1,
            bins: wanted.clone(),
            excitation: Excitation::sine(period, bin, amplitude).faded_in(0.5 * settle.max(1e-3)),
        };
        let (mut tick, delay) = make(rate);
        let periods = simulate_delayed(&run, rate, &mut *tick, delay);
        let s = bins(&periods[1], period, rate, &wanted);
        let h = Harmonics::of(&s, bin, HARMONICS);
        (db(gain(&run.excitation.tones[0], h.fundamental()).norm()), h.thd())
    };
    find_knee(measure, from, 100.0).rounded()
}
