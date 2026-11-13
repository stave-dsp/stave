//! Any hosted plugin as a black box through the characterisation (DSP.md
//! R17–R19): a VST3, an Audio Unit or a catalog stage, behind stave-host's
//! [`Instance`], measured on the same plan, by the same analysers, as a
//! stave model — so two contenders (a stave topology and a third-party
//! plugin, say) can be set side by side.
//!
//! The instance is driven in blocks of the host's size; the block's
//! buffering and the plugin's reported latency are fed through
//! ([`Model::Delayed`]), so the output lines
//! up with its stimulus. Each run starts after [`REST_SECONDS`] of
//! silence (a host cannot reset a plugin through the API). Levels are
//! relative to each contender's own knee (principle 3): the comparison
//! needs no common calibration between a plugin's full scale and a
//! circuit's volts.
//!
//! A black box is a benchmark, not a reference: nothing here holds a
//! model to a third-party plugin, and its tables are never committed.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;

use stave_host::instance::{Instance, Tail};
use stave_measure::perceptual::rnonlin::rnonlin;
use stave_measure::perceptual::tiled;
use stave_measure::suite::analysis::{gain, phasor, waveform, Harmonics, Spectrum};
use stave_measure::suite::levels::{Criterion, Knee};
use stave_measure::suite::db;

use super::fingerprint::{stage_plan, STAGE_SETTLE_SECONDS};
use super::model::{delayed_knee, Model, Ticker};
use super::plan::{Kind, Plan, Tier, HARMONICS};
use super::table::Table;

/// The silence a black box gets before each run.
pub const REST_SECONDS: f64 = 0.1;

/// The quietest level the knee search tries, dBFS
/// ([`stage_knee`](super::model::stage_knee) starts at 10 µ of full scale).
pub const KNEE_FLOOR_DB: f64 = -100.0;

/// A shared instance as a [`Maker`](super::model::Maker): each fresh ticker plays
/// [`REST_SECONDS`] of silence, then takes one sample at a time, processes
/// whole blocks of `block` frames (mono in to every input, the first
/// output out) and answers `block − 1 + latency` samples late.
pub fn maker(instance: Rc<RefCell<Box<dyn Instance + Send>>>, block: usize) -> impl Fn(f64) -> (Ticker, usize) {
    move |rate| {
        let (layout, latency) = {
            let i = instance.borrow();
            (i.layout(), i.latency() as usize)
        };
        let block = block.clamp(1, layout.max_block.max(1));
        let (n_in, n_out) = (layout.inputs.max(1), layout.outputs.max(1));
        let inst = instance.clone();
        let mut ins = vec![vec![0.0f32; block]; n_in];
        let mut outs = vec![vec![0.0f32; block]; n_out];
        let run = move |ins: &[Vec<f32>], outs: &mut [Vec<f32>]| {
            let in_refs: Vec<&[f32]> = ins.iter().map(|b| &b[..]).collect();
            let mut out_refs: Vec<&mut [f32]> = outs.iter_mut().map(|b| &mut b[..]).collect();
            inst.borrow_mut().process(&in_refs, &mut out_refs, &[]);
        };
        for _ in 0..((REST_SECONDS * rate) as usize).div_ceil(block) {
            run(&ins, &mut outs);
        }
        let mut ready = vec![0.0f32; block];
        let mut at = 0usize;
        let tick = move |x: f64| {
            for c in &mut ins {
                c[at] = x as f32;
            }
            if at == block - 1 {
                run(&ins, &mut outs);
                ready.copy_from_slice(&outs[0]);
            }
            at = (at + 1) % block;
            f64::from(ready[at % block])
        };
        (Box::new(tick) as Ticker, block - 1 + latency)
    }
}

/// What makes a black box unusable as a contender.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Inert {
    /// Its output is its input, sample for sample (a bypassed or
    /// unlicensed plugin passing audio through).
    Identity,
    /// No output.
    Silent,
}

/// One contender characterised at one rate.
pub struct BlackBox {
    pub rate: f64,
    pub block: usize,
    pub latency: u32,
    pub tail: Tail,
    pub knee: Knee,
    pub plan: Plan,
    pub tier: Tier,
    pub table: Table,
    /// Wall time of `process` per sample, processing the knee-level
    /// multisine in blocks of `block`.
    pub ns_per_sample: f64,
    pub inert: Option<Inert>,
}

/// Characterise `instance` (opened at `rate` with `max_block ≥ block`):
/// its knee at 1 kHz, the plan's `tier` at that knee, its CPU cost.
pub fn characterise(instance: Box<dyn Instance + Send>, rate: f64, block: usize, tier: Tier) -> BlackBox {
    let (latency, tail) = (instance.latency(), instance.tail());
    let shared = Rc::new(RefCell::new(instance));
    let make = maker(shared.clone(), block);
    let knee = delayed_knee(&make, rate, STAGE_SETTLE_SECONDS);
    let plan = stage_plan("black box", knee, STAGE_SETTLE_SECONDS);
    let model = Model::Delayed { make: Box::new(make) };
    let (table, _) = model.measure(&plan, &stave_dsp::circuit::netlist::Netlist::new(), tier, rate, false, super::Shipping::PLAIN, &[]);
    let ns_per_sample = cost(&mut **shared.borrow_mut(), &plan, rate, block);
    let inert = inert(&plan, &table);
    BlackBox {
        rate,
        block,
        latency,
        tail,
        knee,
        plan,
        tier,
        table,
        ns_per_sample,
        inert,
    }
}

/// `process`'s wall time per sample over two seconds of the knee-level
/// multisine (after a quarter second's warm-up).
pub fn cost(instance: &mut dyn Instance, plan: &Plan, rate: f64, block: usize) -> f64 {
    let Some(run) = plan.run("multisine/+0/r1") else { return f64::NAN };
    let layout = instance.layout();
    let block = block.clamp(1, layout.max_block.max(1));
    let mut ins = vec![vec![0.0f32; block]; layout.inputs.max(1)];
    let mut outs = vec![vec![0.0f32; block]; layout.outputs.max(1)];
    let (warm, timed) = ((0.25 * rate) as usize, (2.0 * rate) as usize);
    let mut spent = std::time::Duration::ZERO;
    let mut i = 0usize;
    while i < warm + timed {
        for k in 0..block {
            let x = run.excitation.at((i + k) as f64 / rate) as f32;
            for c in &mut ins {
                c[k] = x;
            }
        }
        let in_refs: Vec<&[f32]> = ins.iter().map(|b| &b[..]).collect();
        let mut out_refs: Vec<&mut [f32]> = outs.iter_mut().map(|b| &mut b[..]).collect();
        let t = std::time::Instant::now();
        instance.process(&in_refs, &mut out_refs, &[]);
        if i >= warm {
            spent += t.elapsed();
        }
        i += block;
    }
    spent.as_nanos() as f64 / timed as f64
}

/// Identity: unity small-signal gain everywhere and no harmonic above
/// −120 dBc at the hottest 1 kHz tone. Silent: no small-signal gain anywhere.
fn inert(plan: &Plan, table: &Table) -> Option<Inert> {
    if table.ac.is_empty() {
        return None;
    }
    if table.ac.iter().all(|(_, g)| g.norm() < 1e-5) {
        return Some(Inert::Silent);
    }
    // By magnitude: a pass-through that reports a latency it does not
    // have still reads as one, phase-shifted by the false compensation.
    let unity = table.ac.iter().all(|(_, g)| (g.norm() - 1.0).abs() < 1e-4);
    let clean = tones(plan, table, 1_000.0).last().is_none_or(|t| t.harmonics.iter().all(|h| *h < -120.0));
    (unity && clean).then_some(Inert::Identity)
}

/// One tone run, read.
#[derive(Clone, Debug)]
pub struct ToneReading {
    pub offset_db: f64,
    /// The fundamental's gain, dB.
    pub gain_db: f64,
    /// H2…H15, dBc (−200 for none).
    pub harmonics: Vec<f64>,
    pub thd: f64,
    pub thdn: Option<f64>,
    pub floor_dbc: Option<f64>,
    pub anmr: Option<f64>,
}

impl ToneReading {
    /// Even harmonics' power against odd harmonics', dB.
    pub fn even_over_odd_db(&self) -> f64 {
        let power = |even: bool| -> f64 {
            self.harmonics
                .iter()
                .enumerate()
                .filter(|(i, _)| ((i + 2) % 2 == 0) == even)
                .map(|(_, d)| 10f64.powf(d / 10.0))
                .sum()
        };
        10.0 * (power(true) / power(false).max(1e-30)).log10()
    }
}

/// The tone runs at `hz` (by the plan's tag), coolest first.
pub fn tones(plan: &Plan, table: &Table, hz: f64) -> Vec<ToneReading> {
    let tag = match hz as u32 {
        1_000 => "1k".to_string(),
        3_000 => "3k".into(),
        5_000 => "5k".into(),
        8_000 => "8k".into(),
        f => f.to_string(),
    };
    let mut out: Vec<ToneReading> = plan
        .runs
        .iter()
        .filter(|r| r.id.starts_with(&format!("tone/{tag}/")))
        .filter_map(|run| {
            let Kind::Tone { bin, offset_db } = run.kind else { return None };
            let m = table.runs.get(&run.id)?;
            let h = Harmonics::of(&m.periods[0], bin, HARMONICS);
            Some(ToneReading {
                offset_db,
                gain_db: db(gain(&run.excitation.tones[0], h.fundamental()).norm()),
                harmonics: (2..=HARMONICS).map(|k| h.dbc(k).unwrap_or(-200.0).max(-200.0)).collect(),
                thd: h.thd(),
                thdn: m.thdn,
                floor_dbc: m.floor.map(db),
                anmr: m.anmr,
            })
        })
        .collect();
    out.sort_by(|a, b| a.offset_db.total_cmp(&b.offset_db));
    out
}

/// Rnonlin (Tan & Moore) of each multisine level's first realisation:
/// what went in against what came out, 1 for no distortion.
pub fn rnonlins(plan: &Plan, table: &Table, rate: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for run in plan.runs.iter().filter(|r| r.id.ends_with("/r1")) {
        let Kind::Multisine { offset_db, .. } = run.kind else { continue };
        let Some(m) = table.runs.get(&run.id) else { continue };
        let n = run.period().samples(rate);
        let below = |s: Spectrum| Spectrum {
            bins: s.bins.into_iter().filter(|(k, _)| 2 * (*k as usize) < n).collect(),
        };
        let input = below(Spectrum {
            bins: run.excitation.tones.iter().map(|t| (t.bin, phasor(t))).collect(),
        });
        let x = waveform(&input, run.period(), rate);
        let y = waveform(&m.periods[0], run.period(), rate);
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let y: Vec<f64> = y.iter().map(|v| v - mean).collect();
        let len = (0.2 * rate) as usize;
        out.push((offset_db, rnonlin(&tiled(&x, len), &tiled(&y, len), rate).value));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// The small-signal gain at the response point nearest `hz`, dB.
pub fn response_db(table: &Table, hz: f64) -> Option<f64> {
    table
        .ac
        .iter()
        .min_by(|a, b| (a.0 / hz).ln().abs().total_cmp(&(b.0 / hz).ln().abs()))
        .map(|(_, g)| db(g.norm()))
}

/// The worst (highest) ANMR over every tone run at `offset_db`, and where.
pub fn worst_anmr(plan: &Plan, table: &Table, offset_db: f64) -> Option<(f64, String)> {
    plan.runs
        .iter()
        .filter(|r| matches!(r.kind, Kind::Tone { offset_db: o, .. } if o == offset_db))
        .filter_map(|r| Some((table.runs.get(&r.id)?.anmr?, r.id.clone())))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

fn criterion(c: Criterion) -> &'static str {
    match c {
        Criterion::Compression => "1 dB compression",
        Criterion::Distortion => "1% THD",
        Criterion::Unreached => "unreached (no knee up to +40 dBFS)",
    }
}

fn cell(x: Option<f64>, digits: usize) -> String {
    match x {
        Some(v) if v.is_finite() => format!("{v:.digits$}"),
        _ => "—".into(),
    }
}

/// A markdown table of `contenders` (name, characterisation), all at one
/// rate, metric by metric.
pub fn side_by_side(contenders: &[(String, &BlackBox)]) -> String {
    let mut s = String::new();
    let head: Vec<&str> = contenders.iter().map(|(n, _)| n.as_str()).collect();
    let _ = writeln!(s, "| | {} |", head.join(" | "));
    let _ = writeln!(s, "|---|{}", "---|".repeat(head.len()));
    let mut row = |label: &str, f: &dyn Fn(&BlackBox) -> String| {
        let cells: Vec<String> = contenders.iter().map(|(_, b)| f(b)).collect();
        let _ = writeln!(s, "| {label} | {} |", cells.join(" | "));
    };
    row("inert", &|b| match b.inert {
        None => "no".into(),
        Some(Inert::Identity) => "**identity (passes input unchanged)**".into(),
        Some(Inert::Silent) => "**silent**".into(),
    });
    row("knee @ 1 kHz (dBFS peak)", &|b| {
        if db(b.knee.amplitude) <= KNEE_FLOOR_DB + 0.05 {
            // The search's first, quietest step was already past the
            // criterion: a gate, modelled noise or a latch, not a knee.
            format!("≤ {KNEE_FLOOR_DB:+.0} (past {} at the quietest level: a gate, noise or a latch?)", criterion(b.knee.criterion))
        } else {
            format!("{:+.1} ({})", db(b.knee.amplitude), criterion(b.knee.criterion))
        }
    });
    row("latency (samples)", &|b| b.latency.to_string());
    row("tail", &|b| match b.tail {
        Tail::Samples(n) => format!("{n} samples"),
        Tail::Infinite => "infinite".into(),
    });
    row("CPU (ns/sample)", &|b| format!("{:.0} (block {})", b.ns_per_sample, b.block));
    row("CPU (% of one core in real time)", &|b| format!("{:.2}", b.ns_per_sample * b.rate / 1e7));
    row("small-signal gain @ 1 kHz (dB, −40 re knee)", &|b| cell(response_db(&b.table, 1_000.0), 2));
    for hz in [50.0, 100.0, 300.0, 3_000.0, 6_000.0, 10_000.0, 16_000.0] {
        row(&format!("response @ {hz} Hz re 1 kHz (dB)"), &|b| {
            cell(response_db(&b.table, hz).zip(response_db(&b.table, 1_000.0)).map(|(a, r)| a - r), 2)
        });
    }
    let offsets: Vec<f64> = contenders
        .first()
        .map(|(_, b)| tones(&b.plan, &b.table, 1_000.0).iter().map(|t| t.offset_db).collect())
        .unwrap_or_default();
    for o in &offsets {
        let at = |b: &BlackBox| tones(&b.plan, &b.table, 1_000.0).into_iter().find(|t| t.offset_db == *o);
        let small = |b: &BlackBox| tones(&b.plan, &b.table, 1_000.0).first().map(|t| t.gain_db);
        row(&format!("1 kHz {o:+} dB re knee: compression (dB)"), &|b| {
            cell(at(b).map(|t| t.gain_db).zip(small(b)).map(|(g, s)| g - s), 2)
        });
        row(&format!("1 kHz {o:+}: H2 / H3 / H5 (dBc)"), &|b| {
            at(b).map_or("—".into(), |t| format!("{:.1} / {:.1} / {:.1}", t.harmonics[0], t.harmonics[1], t.harmonics[3]))
        });
        row(&format!("1 kHz {o:+}: THD (%) / even−odd (dB)"), &|b| {
            at(b).map_or("—".into(), |t| format!("{:.3} / {:+.1}", 100.0 * t.thd, t.even_over_odd_db()))
        });
        row(&format!("1 kHz {o:+}: alias floor (dBc) / ANMR (dB)"), &|b| {
            at(b).map_or("—".into(), |t| format!("{} / {}", cell(t.floor_dbc, 1), cell(t.anmr, 1)))
        });
    }
    for o in [12.0, 20.0] {
        row(&format!("worst ANMR over tones at {o:+} dB re knee (dB)"), &|b| {
            worst_anmr(&b.plan, &b.table, o).map_or("—".into(), |(a, at)| format!("{a:.1} ({at})"))
        });
    }
    row("Rnonlin by multisine level (dB re knee: value)", &|b| {
        let r: Vec<String> = rnonlins(&b.plan, &b.table, b.rate).iter().map(|(o, v)| format!("{o:+}: {v:.3}")).collect();
        r.join(", ")
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_host::instance::Layout;
    use stave_measure::suite::Complex;

    /// A test double: `y = tanh(drive·x)` delayed by `latency`, mono.
    struct Shaper {
        drive: f32,
        latency: usize,
        line: std::collections::VecDeque<f32>,
        max_block: usize,
    }

    impl Instance for Shaper {
        fn layout(&self) -> Layout {
            Layout {
                inputs: 1,
                outputs: 1,
                sample_rate: 48_000.0,
                max_block: self.max_block,
            }
        }
        fn latency(&self) -> u32 {
            self.latency as u32
        }
        fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[stave_core::Event]) {
            for (x, y) in inputs[0].iter().zip(outputs[0].iter_mut()) {
                self.line.push_back((self.drive * x).tanh());
                *y = if self.line.len() > self.latency { self.line.pop_front().unwrap() } else { 0.0 };
            }
        }
    }

    fn shaper(drive: f32, latency: usize, max_block: usize) -> Box<dyn Instance + Send> {
        Box::new(Shaper {
            drive,
            latency,
            line: Default::default(),
            max_block,
        })
    }

    /// Blocks and latency are fed through: a latent, block-processed
    /// shaper measures as the same shaper ticked a sample at a time.
    #[test]
    fn block_size_and_latency_do_not_change_the_measurement() {
        let a = characterise(shaper(2.0, 0, 1), 48_000.0, 1, Tier::T0);
        let b = characterise(shaper(2.0, 37, 64), 48_000.0, 64, Tier::T0);
        assert_eq!(a.knee, b.knee);
        assert_eq!((a.latency, b.latency), (0, 37));
        let (ta, tb) = (tones(&a.plan, &a.table, 1_000.0), tones(&b.plan, &b.table, 1_000.0));
        assert_eq!(ta.len(), 3);
        for (x, y) in ta.iter().zip(&tb) {
            assert!((x.gain_db - y.gain_db).abs() < 1e-4, "{x:?} {y:?}");
            assert!((x.harmonics[1] - y.harmonics[1]).abs() < 1e-3, "{x:?} {y:?}");
        }
        for ((f, g), (_, h)) in a.table.ac.iter().zip(&b.table.ac) {
            assert!(Complex::new(g.re - h.re, g.im - h.im).norm() < 1e-4, "{f} Hz: {g:?} vs {h:?}");
        }
        assert!(a.inert.is_none());
        assert!(tb[2].harmonics[1] > -40.0, "tanh at +12 dB re knee is hot: {:?}", tb[2]);
        assert!(tb[2].even_over_odd_db() < -60.0, "tanh is odd: {:?}", tb[2]);
    }

    /// A wire is flagged as one, even when it reports a latency it does
    /// not have (as a plugin passing audio through unprocessed can).
    #[test]
    fn a_wire_is_inert() {
        struct Wire;
        impl Instance for Wire {
            fn latency(&self) -> u32 {
                88
            }
            fn layout(&self) -> Layout {
                Layout {
                    inputs: 1,
                    outputs: 1,
                    sample_rate: 48_000.0,
                    max_block: 32,
                }
            }
            fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[stave_core::Event]) {
                o[0].copy_from_slice(i[0]);
            }
        }
        let w = characterise(Box::new(Wire), 48_000.0, 32, Tier::T0);
        assert_eq!(w.inert, Some(Inert::Identity));
        assert_eq!(w.knee.criterion, Criterion::Unreached);
        let table = side_by_side(&[("wire".into(), &w)]);
        assert!(table.contains("identity"), "{table}");
    }
}
