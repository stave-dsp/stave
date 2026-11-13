//! Classic circuits (`circuit::topologies`) and the ladder filters as
//! catalog stages, so a host tool can play, measure and audition the
//! verified reference models beside any plugin: `Circuit<Dk>` on the
//! unit's netlist, in volts, inside an oversampled region of the chosen
//! factor.
//!
//! These are the reference models, not shipping plugins: whole netlists
//! cost microseconds a sample (the datasheet's "`Circuit<Dk>` …" lines).
//! Every variant a setting can select (oversampling factor, the 904-A's
//! regeneration) is built with the stage, so a param change only picks
//! another one — no allocation at the block boundary.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::topologies::{marshall_jcm800_2203, moog_ladder_904a, tube_screamer_ts808};
use stave_dsp::circuit::Circuit;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::units::{db, hz};
use stave_dsp::{NewtonStats, Oversampled, Stage, Tail};

use super::catalog::{choice, float, stage_with_solver};
use super::{BoxedStage, StageParams};
use stave_core::params::Unit;

/// The oversampling factors a classic offers, by [`Factor`]'s index.
pub const FACTORS: [u32; 4] = [1, 2, 4, 8];

options_list!(
    /// The oversampling factor.
    Factor = ["1x", "2x", "4x", "8x"]
);

/// A netlist in volts: audio × `full_scale_in` drives its input source,
/// the probe's departure from its resting value ÷ `full_scale_out` comes
/// out.
#[derive(Clone)]
pub struct Volts {
    circuit: Circuit<Dk>,
    rest: f64,
    pub full_scale_in: f64,
    pub full_scale_out: f64,
}

impl Volts {
    fn new(netlist: &Netlist, source: &str) -> Self {
        let mut circuit = Circuit::<Dk>::new(netlist).unwrap_or_else(|e| panic!("a topology does not build: {e:?}"));
        circuit.drive(source).unwrap_or_else(|e| panic!("a topology has no source {source}: {e:?}"));
        let rest = circuit.output();
        Self {
            circuit,
            rest,
            full_scale_in: 1.0,
            full_scale_out: 1.0,
        }
    }
}

impl Stage for Volts {
    fn prepare(&mut self, sample_rate: f32) {
        self.circuit.prepare(sample_rate);
        self.rest = self.circuit.output();
    }
    fn reset(&mut self) {
        self.circuit.reset();
        self.rest = self.circuit.output();
    }
    fn tick(&mut self, x: f32) -> f32 {
        let v = self.circuit.tick_f64(f64::from(x) * self.full_scale_in);
        ((v - self.rest) / self.full_scale_out) as f32
    }
    fn tail(&self) -> Tail {
        self.circuit.tail()
    }
}

/// Every prebuilt variant of one circuit, the selected one running.
pub struct Variants<S: Stage> {
    variants: Vec<Oversampled<S>>,
    selected: usize,
}

impl<S: Stage + Clone> Variants<S> {
    /// `bases[i]` at every factor of [`FACTORS`]: variant `i * 4 + f`.
    fn new(bases: Vec<S>) -> Self {
        let variants = bases
            .into_iter()
            .flat_map(|b| FACTORS.map(|f| Oversampled::linear(b.clone(), f)))
            .collect();
        Self { variants, selected: 0 }
    }

    fn select(&mut self, index: usize) {
        let index = index.min(self.variants.len() - 1);
        if index != self.selected {
            self.selected = index;
            self.variants[index].reset();
        }
    }

    fn each(&mut self, f: impl Fn(&mut S)) {
        for v in &mut self.variants {
            f(&mut v.inner);
        }
    }

    fn current(&self) -> &Oversampled<S> {
        &self.variants[self.selected]
    }
}

impl<S: Stage + Clone> Stage for Variants<S> {
    fn prepare(&mut self, sample_rate: f32) {
        for v in &mut self.variants {
            v.prepare(sample_rate);
        }
    }
    fn reset(&mut self) {
        self.variants[self.selected].reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.variants[self.selected].tick(x)
    }
    fn latency(&self) -> u32 {
        self.current().latency()
    }
    fn tail(&self) -> Tail {
        self.current().tail()
    }
}

/// The factor, and the volts at full scale in and out: `(default,
/// most)` each, the most being the circuit's documented valid input
/// range (past it the solver is outside what the topology claims).
fn volts_params(fs_in: (f64, f64), fs_out: (f64, f64)) -> Vec<Box<dyn stave_core::params::AnyParam>> {
    vec![
        choice::<Factor>(0, "oversample"),
        float(1, "full_scale_in", 0.001, fs_in.1, fs_in.0, Unit::Custom { label: "V/FS", decimals: 3 }),
        float(2, "full_scale_out", 0.001, fs_out.1, fs_out.0, Unit::Custom { label: "V/FS", decimals: 3 }),
    ]
}

fn configure_volts(c: &mut Variants<Volts>, v: &StageParams, base: usize) {
    let (fin, fout) = (v.get("full_scale_in"), v.get("full_scale_out"));
    c.each(|s| {
        s.full_scale_in = fin;
        s.full_scale_out = fout;
    });
    c.select(base * FACTORS.len() + v.choice("oversample"));
}

fn volts_stats(c: &Variants<Volts>) -> NewtonStats {
    c.current().inner.circuit.stats()
}

/// One circuit, its scale and factor from the params.
pub fn netlist_stage(netlist: Netlist, v: &StageParams) -> BoxedStage {
    let mut s = Variants::new(vec![Volts::new(&netlist, "vin")]);
    configure_volts(&mut s, v, 0);
    stage_with_solver(s, |c, v| configure_volts(c, v, 0), volts_stats)
}

pub fn ts808_params() -> StageParams {
    StageParams::new(volts_params((1.0, 4.5), (1.0, 10.0)))
}

pub fn ts808(v: &StageParams) -> BoxedStage {
    netlist_stage(tube_screamer_ts808(), v)
}

pub fn jcm800_params() -> StageParams {
    StageParams::new(volts_params((1.0, 5.0), (50.0, 300.0)))
}

pub fn jcm800(v: &StageParams) -> BoxedStage {
    netlist_stage(marshall_jcm800_2203(), v)
}

/// The 904-A's regeneration settings offered (each its own netlist).
pub const MOOG_K: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 3.5];

options_list!(
    /// The 904-A's ideal regeneration `k`.
    MoogK = ["k 0", "k 1", "k 2", "k 3", "k 3.5"]
);

pub fn moog_params() -> StageParams {
    let mut p = volts_params((0.1, 0.3), (0.1, 1.0));
    p.push(choice::<MoogK>(3, "regeneration"));
    StageParams::new(p)
}

fn configure_moog(c: &mut Variants<Volts>, v: &StageParams) {
    configure_volts(c, v, v.choice("regeneration"));
}

pub fn moog(v: &StageParams) -> BoxedStage {
    let bases = MOOG_K.iter().map(|&k| Volts::new(&moog_ladder_904a(k), "vin")).collect();
    let mut s = Variants::new(bases);
    configure_moog(&mut s, v);
    stage_with_solver(s, configure_moog, volts_stats)
}

options_list!(
    /// The ladder model.
    LadderModel = ["Huovilainen", "Stinchcombe"]
);

/// Both ladder models, one running.
#[derive(Clone)]
pub struct Ladders {
    huovilainen: Huovilainen,
    stinchcombe: Stinchcombe,
    stinch: bool,
}

impl Stage for Ladders {
    fn prepare(&mut self, sample_rate: f32) {
        self.huovilainen.prepare(sample_rate);
        self.stinchcombe.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.huovilainen.reset();
        self.stinchcombe.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.stinch {
            self.stinchcombe.tick(x)
        } else {
            self.huovilainen.tick(x)
        }
    }
}

pub fn ladder_params() -> StageParams {
    StageParams::new(vec![
        choice::<LadderModel>(0, "model"),
        choice::<Factor>(1, "oversample"),
        float(2, "cutoff", 20.0, 20_000.0, 1_000.0, Unit::Custom { label: "Hz", decimals: 1 }),
        float(3, "resonance", 0.0, 1.0, 0.0, Unit::Plain { decimals: 3 }),
        float(4, "drive", -24.0, 36.0, 0.0, Unit::Custom { label: "dB", decimals: 1 }),
    ])
}

fn configure_ladder(c: &mut Variants<Ladders>, v: &StageParams) {
    let (cutoff, res, drive, stinch) = (v.f32("cutoff"), v.f32("resonance"), v.f32("drive"), v.choice("model") == 1);
    c.each(|l| {
        l.huovilainen.set_cutoff(hz(cutoff));
        l.huovilainen.set_resonance(res);
        l.huovilainen.set_drive(db(drive));
        l.stinchcombe.set_cutoff(hz(cutoff));
        l.stinchcombe.set_resonance(res);
        l.stinchcombe.set_drive(db(drive));
        l.stinch = stinch;
    });
    c.select(v.choice("oversample"));
}

pub fn ladder(v: &StageParams) -> BoxedStage {
    let base = Ladders {
        huovilainen: Huovilainen::new(hz(1_000.0), 0.0),
        stinchcombe: Stinchcombe::new(hz(1_000.0), 0.0),
        stinch: false,
    };
    let mut s = Variants::new(vec![base]);
    configure_ladder(&mut s, v);
    stage_with_solver(s, configure_ladder, |c| {
        let l = &c.current().inner;
        if l.stinch {
            l.stinchcombe.stats()
        } else {
            l.huovilainen.stats()
        }
    })
}
