//! The model side: a circuit (`Circuit<Dk>` on the deck) or a hand
//! topology (any ticking stage), run through a plan at one rate into a
//! [`Table`] of the same measurements the reference holds.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;
use stave_dsp::kernel::node::Tail;
use stave_measure::suite::analysis::{
    aliasing, bins, gain, steady_state, thd_n, Harmonics, Spectrum,
};
use stave_measure::suite::excitation::Excitation;
use stave_measure::suite::levels::{find_knee, Knee};
use stave_measure::suite::period::{harmonic_bins, tone_bin, Period, SHIPPING_RATES};
use stave_measure::suite::{db, Complex};

use super::compare::averaged_bla;
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
    let (netlist, driven) = split(deck, input);
    let mut c = Circuit::<Dk>::new(&netlist).unwrap_or_else(|e| panic!("{e:?}"));
    c.drive(&driven).unwrap();
    c.prepare(rate as f32);
    c
}

/// A fresh ticking model: the stimulus at `i / rate` in, the output then out.
pub type Ticker = Box<dyn FnMut(f64) -> f64>;

/// What a plan is run on.
pub enum Model {
    /// `Circuit<Dk>` on a netlist (the deck, or the deck with stave's
    /// devices in), split by the plan's input.
    Circuit(Netlist),
    /// A hand topology: `make(rate)` returns a fresh ticker, called with
    /// the stimulus at `i / rate` for `i = 0, 1, …` and returning the
    /// output then; `operating_point` lists the node voltages it knows.
    Stage {
        make: Box<dyn Fn(f64) -> Ticker>,
        operating_point: Vec<(String, f64)>,
    },
}

/// One run at `rate`: the measured periods and the one before them.
fn simulate(run: &Run, rate: f64, tick: &mut dyn FnMut(f64) -> f64) -> Vec<Vec<f64>> {
    let n = run.period().samples(rate);
    let total = (run.settle + run.measure) as usize * n;
    let keep = (run.settle as usize - 1) * n;
    let mut out = Vec::with_capacity(total - keep);
    for i in 0..total {
        let y = tick(run.excitation.at(i as f64 / rate));
        if i >= keep {
            out.push(y);
        }
    }
    out.chunks(n).map(<[f64]>::to_vec).collect()
}

/// A run's measurements from its simulated periods (the first is the
/// one before the measured ones).
pub fn measure_run(run: &Run, rate: f64, periods: &[Vec<f64>]) -> Measured {
    let p = run.period();
    let spectra: Vec<Spectrum> = periods[1..]
        .iter()
        .map(|x| bins(x, p, rate, &run.bins))
        .collect();
    let (thdn, floor) = match run.kind {
        Kind::Tone { bin, .. } => {
            let last = &periods[1];
            (Some(thd_n(last, p, rate, bin)), Some(aliasing(last, p, rate, bin)))
        }
        _ => (None, None),
    };
    Measured {
        steady: steady_state(&periods[0], &periods[1]),
        thdn,
        floor,
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
    /// operating-point nodes to read (the reference's).
    pub fn measure(
        &self,
        plan: &Plan,
        deck: &Netlist,
        tier: Tier,
        rate: f64,
        oversampled: bool,
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
                let base = circuit(netlist, &plan.input, rate);
                let uic = netlist.tran.is_some_and(|t| t.uic);
                if !uic {
                    let (split_netlist, _) = split(netlist, &plan.input);
                    for node in nodes {
                        if let Some(n) = split_netlist.node(node) {
                            let mut c = base.clone();
                            c.set_probe(Probe::node(n));
                            table.op.push((node.clone(), c.output()));
                        }
                    }
                }
                for &hz in &plan.response_hz {
                    if hz < rate / 2.0 {
                        let r = if oversampled {
                            base.analog_response(hz)
                        } else {
                            base.frequency_response(hz)
                        };
                        if let Some(r) = r {
                            table.ac.push((hz, Complex::new(r.re, r.im)));
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
                            c.tick_f64(x)
                        }
                    };
                    let periods = simulate(run, rate, &mut tick);
                    let stats = c.stats();
                    health.capped += stats.capped;
                    health.solves += stats.solves;
                    table.runs.insert(run.id.clone(), measure_run(run, rate, &periods));
                }
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
                    table.runs.insert(run.id.clone(), measure_run(run, rate, &periods));
                }
                // C2 from the −40 dB multisine's BLA (averaged over its
                // realisations) at the response points.
                if let Some(bla) = averaged_bla(plan, tier, &table, -40.0) {
                    table.ac = bla
                        .into_iter()
                        .map(|(k, g)| (super::plan::MULTISINE_PERIOD.hz(k), g))
                        .collect();
                }
            }
        }
        let _ = deck;
        (table, health)
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
