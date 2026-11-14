//! The classic pedals as reference stages with their front panels: each
//! verified netlist (`circuit::topologies`, or a verified deck of
//! `stave-test/spice/`) with its pots declared variable, so a knob
//! setting glides the running circuit there (`circuit::variable`, a
//! low-rank update, no rebuild and no allocation) instead of picking a
//! prebuilt netlist.
//!
//! Every pedal takes the classics' `oversample`, `full_scale_in` and
//! `full_scale_out` (volts at digital full scale), then its knobs, each
//! 0..=1 as the pot's rotation (0.5 is noon), with the pot's taper
//! applied. A knob absent from a pedal's list sits where its topology's
//! docs say (volume pots at maximum: level is `full_scale_out`'s).
//!
//! | stage | knobs | other settings |
//! |---|---|---|
//! | `circuit/ts808` | drive, tone | opamp: JRC4558 (fitted) or JRC4558 (table) |
//! | `circuit/ts808_lfo` | drive, tone | opamp; lfo_target, lfo_rate, lfo_depth, step_ms, glide_ms |
//! | `circuit/klon_centaur` | gain, treble | |
//! | `circuit/mxr_distortion_plus` | distortion, output | |
//! | `circuit/boss_ds1` | distortion, tone | |
//! | `circuit/proco_rat` | distortion, filter | |
//! | `circuit/big_muff` | tone | |
//! | `circuit/fuzz_face` | fuzz | |
//! | `circuit/rangemaster` | | |
//! | `circuit/phase90` | | mode: swept or frozen; rate (swept), gate (frozen) |
//!
//! `circuit/ts808_lfo` is the knob-smoothing audition: an LFO inside the
//! stage turns drive, tone or both `lfo_depth` either side of where they
//! are set, re-setting the pots every `step_ms` (an automation lane's
//! block: 1.333 ms is 64 samples at 48 kHz); each step reaches its value
//! over `glide_ms` (0 jumps: zipper noise; the framework's default glide
//! is 5 ms).

use stave_core::params::{AnyParam, Unit};
use stave_dsp::circuit::devices::opamp::{Amp, Macromodel};
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Taper, Variable};
use stave_dsp::circuit::topologies::tone_stacks::POT_END_OHMS;
use stave_dsp::circuit::topologies::{
    big_muff_rams_head, boss_ds1, klon_centaur, mxr_distortion_plus, mxr_phase90, proco_rat, tube_screamer_ts808,
};
use stave_dsp::{NewtonStats, Stage, Tail};

use super::catalog::{choice, float, stage_with_solver};
use super::classics::{configure_volts, volts_params, Variants};
use super::volts::{Lfo, Sweep, Volts};
use super::{BoxedStage, StageParams};

/// A front-panel knob: its setting, its default rotation, and the pots
/// it turns (by their [`Variable`] names, `-` before one it turns the
/// other way).
pub struct Knob {
    pub setting: &'static str,
    pub default: f64,
    pub pots: &'static [&'static str],
}

/// A pedal: its netlists (one per value of the setting `base` reads),
/// its knobs, and the settings beyond them.
pub struct Pedal {
    /// Default and most volts at full scale, in and out.
    fs_in: (f64, f64),
    fs_out: (f64, f64),
    bases: fn() -> Vec<Netlist>,
    knobs: &'static [Knob],
    /// The settings after the knobs, from the id given.
    extra_params: fn(u32) -> Vec<Box<dyn AnyParam>>,
    /// Which netlist the settings select.
    base: fn(&StageParams) -> usize,
    /// Once, on each netlist before it is cloned into its variants.
    setup: fn(&mut Volts),
    /// The settings beyond the knobs, applied to every variant.
    configure: fn(&mut Volts, &StageParams),
}

const fn pedal(bases: fn() -> Vec<Netlist>, knobs: &'static [Knob]) -> Pedal {
    Pedal {
        fs_in: (1.0, 4.5),
        fs_out: (1.0, 10.0),
        bases,
        knobs,
        extra_params: |_| Vec::new(),
        base: |_| 0,
        setup: |_| {},
        configure: |_, _| {},
    }
}

fn params(p: &Pedal) -> StageParams {
    let mut v = volts_params(p.fs_in, p.fs_out);
    for knob in p.knobs {
        v.push(float(v.len() as u32, knob.setting, 0.0, 1.0, knob.default, Unit::Plain { decimals: 3 }));
    }
    let next = v.len() as u32;
    v.extend((p.extra_params)(next));
    StageParams::new(v)
}

/// A pedal running: its variants and what it is.
struct Running {
    variants: Variants<Volts>,
    pedal: &'static Pedal,
}

impl Running {
    fn configure(&mut self, v: &StageParams) {
        let p = self.pedal;
        self.variants.each(|s| {
            for (k, knob) in p.knobs.iter().enumerate() {
                s.set_knob(k, v.get(knob.setting));
            }
            (p.configure)(s, v);
        });
        configure_volts(&mut self.variants, v, (p.base)(v));
    }
}

impl Stage for Running {
    fn prepare(&mut self, sample_rate: f32) {
        self.variants.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.variants.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.variants.tick(x)
    }
    fn latency(&self) -> u32 {
        self.variants.latency()
    }
    fn tail(&self) -> Tail {
        self.variants.tail()
    }
}

fn build(p: &'static Pedal, v: &StageParams) -> BoxedStage {
    let pots: Vec<&[&str]> = p.knobs.iter().map(|k| k.pots).collect();
    let bases = (p.bases)()
        .iter()
        .map(|n| {
            let mut s = Volts::new(n, "vin", &pots);
            (p.setup)(&mut s);
            s
        })
        .collect();
    let mut running = Running {
        variants: Variants::new(bases),
        pedal: p,
    };
    running.configure(v);
    stage_with_solver(running, Running::configure, |r: &Running| -> NewtonStats {
        r.variants.current().inner.stats()
    })
}

/// `netlist` with `variables` declared.
fn declare(mut netlist: Netlist, variables: impl IntoIterator<Item = Variable>) -> Netlist {
    for variable in variables {
        if let Err(e) = netlist.add_variable(variable) {
            panic!("a pedal declares a pot its netlist cannot take: {e}");
        }
    }
    netlist
}

/// A pot over two legs, the first growing as it turns up.
fn pot(name: &str, legs: [&str; 2], taper: Taper) -> Variable {
    Variable::pot(name, legs, taper).with_end_ohms(POT_END_OHMS)
}

fn rheostat(element: &str, track_ohms: f64, taper: Taper) -> Variable {
    Variable::rheostat(element, track_ohms, taper).with_end_ohms(POT_END_OHMS)
}

macro_rules! pedal_stage {
    ($params:ident, $build:ident, $pedal:ident) => {
        pub fn $params() -> StageParams {
            params(&$pedal)
        }
        pub fn $build(v: &StageParams) -> BoxedStage {
            build(&$pedal, v)
        }
    };
}

// --- Tube Screamer ----------------------------------------------------------

/// The op-amps the TS-808 offers (DSP.md E11): the fitted JRC4558 and
/// the datasheet-table one the ngspice decks write.
pub const TS_OPAMPS: [Macromodel; 2] = [Macromodel::JRC4558, Macromodel::JRC4558_TABLE];

options_list!(
    /// The TS-808's op-amp.
    TsOpAmp = ["JRC4558", "JRC4558 (table)"]
);

/// The TS-808 with both halves of `part`, its drive (500k audio taper in
/// series with the 51k) and tone (20k, toward the feedback side as it
/// turns up: brighter) pots declared.
fn ts808_with(part: Macromodel) -> Netlist {
    let mut netlist = tube_screamer_ts808();
    for name in ["u1a", "u1b"] {
        if let Some(ElementKind::OpAmp { amp, .. }) = netlist.element_mut(name).map(|e| &mut e.kind) {
            *amp = Amp::Macromodel(part);
        }
    }
    declare(
        netlist,
        [
            Variable::rheostat("rf", 500e3, Taper::AUDIO).with_end_ohms(51e3),
            pot("tone", ["rt1", "rt2"], Taper::Linear),
        ],
    )
}

fn ts808_bases() -> Vec<Netlist> {
    TS_OPAMPS.iter().map(|&part| ts808_with(part)).collect()
}

fn ts808_opamp(id: u32) -> Vec<Box<dyn AnyParam>> {
    vec![choice::<TsOpAmp>(id, "opamp")]
}

const TS808_KNOBS: &[Knob] = &[
    Knob { setting: "drive", default: 1.0, pots: &["rf"] },
    Knob { setting: "tone", default: 0.5, pots: &["tone"] },
];

static TS808: Pedal = Pedal {
    extra_params: ts808_opamp,
    base: |v| v.choice("opamp"),
    ..pedal(ts808_bases, TS808_KNOBS)
};
pedal_stage!(ts808_params, ts808, TS808);

options_list!(
    /// Which knobs the automation LFO turns.
    LfoTarget = ["drive", "tone", "both"]
);

static TS808_LFO: Pedal = Pedal {
    extra_params: |id| {
        let ms = Unit::Custom { label: "ms", decimals: 3 };
        vec![
            choice::<TsOpAmp>(id, "opamp"),
            choice::<LfoTarget>(id + 1, "lfo_target"),
            float(id + 2, "lfo_rate", 0.0, 20.0, 1.0, Unit::Hz),
            float(id + 3, "lfo_depth", 0.0, 0.5, 0.4, Unit::Plain { decimals: 3 }),
            float(id + 4, "step_ms", 0.02, 50.0, 1.333, ms),
            float(id + 5, "glide_ms", 0.0, 200.0, 0.0, ms),
        ]
    },
    base: |v| v.choice("opamp"),
    setup: |s| {
        s.sweep = Sweep::Knobs {
            mask: 1,
            depth: 0.0,
            step: 1e-3,
            countdown: 0,
            lfo: Lfo::new(1.0),
        };
    },
    configure: |s, v| {
        s.set_glide(v.get("glide_ms") * 1e-3);
        if let Sweep::Knobs {
            mask,
            depth,
            step,
            lfo,
            ..
        } = &mut s.sweep
        {
            *mask = [0b01, 0b10, 0b11][v.choice("lfo_target").min(2)];
            *depth = v.get("lfo_depth");
            *step = v.get("step_ms") * 1e-3;
            lfo.rate_hz = v.get("lfo_rate");
        }
    },
    ..pedal(
        ts808_bases,
        &[
            Knob { setting: "drive", default: 0.5, pots: &["rf"] },
            Knob { setting: "tone", default: 0.5, pots: &["tone"] },
        ],
    )
};
pedal_stage!(ts808_lfo_params, ts808_lfo, TS808_LFO);

// --- Klon Centaur -----------------------------------------------------------

/// Gain's two gangs (each wiper `gain` × 100k from its top) and treble
/// (10k, toward the 1k8 end as it turns up).
static KLON: Pedal = pedal(
    || {
        vec![declare(
            klon_centaur(0.5, 0.5),
            [
                pot("gain_a", ["rga1", "rga2"], Taper::Linear),
                pot("gain_b", ["rgb1", "rgb2"], Taper::Linear),
                pot("treble", ["rtb", "rta"], Taper::Linear),
            ],
        )]
    },
    &[
        Knob { setting: "gain", default: 0.5, pots: &["gain_a", "gain_b"] },
        Knob { setting: "treble", default: 0.5, pots: &["treble"] },
    ],
);
pedal_stage!(klon_params, klon, KLON);

// --- MXR Distortion+ --------------------------------------------------------

/// The 1M reverse-log distortion rheostat (turning up shorts more of
/// it: an audio law the other way) and the 10k audio output divider.
static DISTORTION_PLUS: Pedal = pedal(
    || {
        vec![declare(
            mxr_distortion_plus(0.5, 0.5),
            [rheostat("rd", 1e6, Taper::AUDIO), pot("output", ["rv2", "rv1"], Taper::AUDIO)],
        )]
    },
    &[
        Knob { setting: "distortion", default: 0.5, pots: &["-rd"] },
        Knob { setting: "output", default: 0.75, pots: &["output"] },
    ],
);
pedal_stage!(distortion_plus_params, distortion_plus, DISTORTION_PLUS);

// --- Boss DS-1 --------------------------------------------------------------

/// The 100k linear distortion rheostat in the gain stage's feedback and
/// the 20k tone pot (toward the high-pass leg as it turns up).
static DS1: Pedal = pedal(
    || {
        vec![declare(
            boss_ds1(),
            [rheostat("rd", 100e3, Taper::Linear), pot("tone", ["rt1", "rt2"], Taper::Linear)],
        )]
    },
    &[
        Knob { setting: "distortion", default: 0.5, pots: &["rd"] },
        Knob { setting: "tone", default: 0.5, pots: &["tone"] },
    ],
);
pedal_stage!(ds1_params, ds1, DS1);

// --- ProCo RAT --------------------------------------------------------------

/// The 100k audio distortion rheostat in feedback and the 100k audio
/// filter rheostat in series with the 1k5 (0 brightest).
static RAT: Pedal = pedal(
    || {
        vec![declare(
            proco_rat(),
            [
                rheostat("rd", 100e3, Taper::AUDIO),
                Variable::rheostat("r6", 100e3, Taper::AUDIO).with_end_ohms(1.5e3),
            ],
        )]
    },
    &[
        Knob { setting: "distortion", default: 0.5, pots: &["rd"] },
        Knob { setting: "filter", default: 0.3, pots: &["r6"] },
    ],
);
pedal_stage!(rat_params, rat, RAT);

// --- Big Muff ---------------------------------------------------------------

/// The Ram's Head's 100k tone pot (toward the high-pass leg as it turns
/// up); sustain and volume at maximum.
static BIG_MUFF: Pedal = pedal(
    || vec![declare(big_muff_rams_head(), [pot("tone", ["rt1", "rt2"], Taper::Linear)])],
    &[Knob { setting: "tone", default: 0.5, pots: &["tone"] }],
);
pedal_stage!(big_muff_params, big_muff, BIG_MUFF);

// --- Fuzz Face and Rangemaster ----------------------------------------------

/// The Arbiter Fuzz Face of `stave-test/spice/fuzz_face.cir` (verified
/// by `transistors_match_ngspice.rs::fuzz_face_matches_ngspice`, 6e-5 at
/// 16×), its 1k fuzz pot written as two legs with the 20u bypass on the
/// wiper (at full fuzz, the deck's circuit), volume at maximum.
const FUZZ_FACE: &str = "Fuzz Face
VIN in 0 0
RG in i 1k
C1 i b1 2.2u
Q1 c1 b1 0 AC128
R1 c1 vee 33k
RFB e2 b1 100k
Q2 c2 c1 e2 AC128
RFZ1 e2 w 500
RFZ2 w 0 500
CFZ w 0 20u
R2 c2 o 470
R3 o vee 8.2k
C2 o out 10n
RV out 0 500k
VEE vee 0 -9
.model AC128 PNP(IS=20.66u BF=229.6 BR=14.66 NF=1.133 NR=1.140 VAF=19.68 VAR=88.28 IKF=463.0m IKR=241.5m ISE=2.190u ISC=7.546u NE=1.796 NC=1.364 RB=1.885 RE=306.4m
+ CJC=200p TF=159.2n)
.control
wrdata out v(out)
.endc";

/// The Dallas Rangemaster of `stave-test/spice/rangemaster.cir`
/// (verified by `transistors_match_ngspice.rs::rangemaster_matches_ngspice`).
const RANGEMASTER: &str = "Rangemaster
VIN in 0 0
C1 in b 5n
RB1 b vee 470k
RB2 b 0 68k
Q1 c b e OC44
RE e 0 3.9k
CE e 0 47u
RC c vee 10k
C2 c out 10n
RL out 0 100k
VEE vee 0 -9
.model OC44 PNP(IS=1.423u BF=307.0 BR=20.27 NF=1.022 NR=1.025 VAF=8.167 VAR=14.84 IKF=43.82m IKR=611.7m ISE=30.54n ISC=213.5n NE=1.316 NC=1.258 RB=32.83 RE=968.7m RC=989.9u
+ CJC=12p TF=19.89n)
.control
wrdata out v(out)
.endc";

fn spice(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap_or_else(|e| panic!("a pedal's netlist does not read: {e}"))
}

static FUZZ: Pedal = pedal(
    || vec![declare(spice(FUZZ_FACE), [pot("fuzz", ["rfz2", "rfz1"], Taper::Linear)])],
    &[Knob { setting: "fuzz", default: 1.0, pots: &["fuzz"] }],
);
pedal_stage!(fuzz_face_params, fuzz_face, FUZZ);

static RANGE: Pedal = pedal(|| vec![spice(RANGEMASTER)], &[]);
pedal_stage!(rangemaster_params, rangemaster, RANGE);

// --- Phase 90 ---------------------------------------------------------------

/// The gate-bus range the swept Phase 90 is verified over
/// (`classics_match_ngspice.rs::mxr_phase90_swept_matches_ngspice`).
pub const PHASE90_SWEEP_VOLTS: (f64, f64) = (3.1, 4.3);

options_list!(
    /// The Phase 90's gate bus: swept by its LFO, or frozen.
    GateMode = ["swept", "frozen"]
);

/// The Phase 90 with its gate bus `vg` held from the stage: a triangle
/// LFO across [`PHASE90_SWEEP_VOLTS`] at `rate`, or frozen at `gate`.
static PHASE90: Pedal = Pedal {
    extra_params: |id| {
        vec![
            choice::<GateMode>(id, "mode"),
            float(id + 1, "rate", 0.05, 10.0, 0.5, Unit::Hz),
            float(id + 2, "gate", 3.0, 5.1, 3.4, Unit::Custom { label: "V", decimals: 3 }),
        ]
    },
    setup: |s| {
        let supply = s.supply("vg");
        s.sweep = Sweep::Gate {
            supply,
            low: PHASE90_SWEEP_VOLTS.0,
            high: PHASE90_SWEEP_VOLTS.1,
            frozen: None,
            lfo: Lfo::new(0.5),
        };
    },
    configure: |s, v| {
        if let Sweep::Gate { frozen, lfo, .. } = &mut s.sweep {
            *frozen = (v.choice("mode") == 1).then(|| v.get("gate"));
            lfo.rate_hz = v.get("rate");
        }
    },
    ..pedal(|| vec![mxr_phase90(3.4)], &[])
};
pedal_stage!(phase90_params, phase90, PHASE90);
