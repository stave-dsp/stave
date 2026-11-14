//! The characterised circuits: each deck's input (the source the plan
//! drives, at its operating-point DC level) and anything held.

use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::topologies;
use stave_test::characterise::{Budget, Characterise, Input, Rates, Shipping, Tier};

pub struct Circuit {
    pub name: &'static str,
    pub input: fn() -> Input,
    /// A supply: its input is the load current, from switch-on.
    pub supply: bool,
    /// A fixed settling time; otherwise half the linearised tail.
    pub settle: Option<f64>,
    /// A classic circuit (DSP.md R24, R25): the catalogued topology's
    /// netlist and its `stave_dsp` path. Its matrix runs that netlist on
    /// the deck's plan, against ngspice on the deck (the deck writes the
    /// topology's op-amps and triodes as behavioural sources; the two are
    /// held together by `classics_match_ngspice.rs`).
    pub classic: Option<Classic>,
}

/// A classic's catalogued topology and its `stave_dsp` path.
pub type Classic = (fn() -> Netlist, &'static str);

const fn circuit(name: &'static str, input: fn() -> Input) -> Circuit {
    Circuit {
        name,
        input,
        supply: false,
        settle: None,
        classic: None,
    }
}

const fn classic(name: &'static str, topology: fn() -> Netlist, catalogue: &'static str) -> Circuit {
    Circuit {
        name,
        input: || Input::new("vin", 0.0),
        supply: false,
        settle: None,
        classic: Some((topology, catalogue)),
    }
}

pub static CIRCUITS: [Circuit; 30] = [
    circuit("diode_clipper", || Input::new("v1", 0.0)),
    circuit("opamp_clipper", || Input::new("v1", 0.0)),
    // The TS808 stage rides on the 4.5 V reference.
    circuit("tube_screamer", || Input::new("v1", 4.5)),
    circuit("common_cathode", || Input::new("v1", 0.0)),
    circuit("common_cathode_grid_current", || Input::new("v1", 0.0)),
    // Fixed bias: the grid at the 12AX7's −1.5 V through the stopper.
    circuit("triode_stage", || Input::new("v1", -1.5)),
    // The class point: the swing x at zero.
    circuit("push_pull", || Input::new("vx", 0.0)),
    circuit("push_pull_pentode", || Input::new("vx", 0.0)),
    // The 20 mA standing load; the plan adds to it. From switch-on, the
    // choke rings for about 0.3 s.
    Circuit {
        name: "power_supply",
        input: || Input::new("il", 0.02),
        supply: true,
        settle: Some(0.35),
        classic: None,
    },
    circuit("fuzz_face", || Input::new("vin", 0.0)),
    circuit("rangemaster", || Input::new("vin", 0.0)),
    circuit("ce_stage", || Input::new("vin", 0.0)),
    circuit("jfet_booster", || Input::new("vin", 0.0)),
    circuit("mosfet_booster", || Input::new("vin", 0.0)),
    // The same transistor decks on their cards' DC part (DSP.md R11).
    circuit("fuzz_face_static", || Input::new("vin", 0.0)),
    circuit("rangemaster_static", || Input::new("vin", 0.0)),
    circuit("ce_stage_static", || Input::new("vin", 0.0)),
    circuit("jfet_booster_static", || Input::new("vin", 0.0)),
    circuit("mosfet_booster_static", || Input::new("vin", 0.0)),
    // The VCA with its control held at 5 V (about 0.28 mA of bias).
    circuit("ota_vca", || Input::new("vin", 0.0).holding("vc", 5.0)),
    // The CE-2's BBD filters (the BBD a unity copy), from IC1A's output.
    circuit("boss_ce2_filters", || Input::new("vin", 0.0)),
    // The classics (DSP.md R24, R25), each its catalogued topology on its
    // transient deck's plan, at the settings that deck documents.
    classic(
        "tube_screamer_ts808",
        topologies::tube_screamer_ts808,
        "stave_dsp::circuit::topologies::tube_screamer::tube_screamer_ts808",
    ),
    classic(
        "big_muff_rams_head",
        topologies::big_muff_rams_head,
        "stave_dsp::circuit::topologies::big_muff::big_muff_rams_head",
    ),
    classic("proco_rat", topologies::proco_rat, "stave_dsp::circuit::topologies::proco_rat::proco_rat"),
    classic(
        "marshall_jcm800_2203",
        topologies::marshall_jcm800_2203,
        "stave_dsp::circuit::topologies::marshall_jcm800::marshall_jcm800_2203",
    ),
    // Regeneration k = 3, the transient deck's.
    classic(
        "moog_ladder_feedback",
        || topologies::moog_ladder_904a(3.0),
        "stave_dsp::circuit::topologies::moog_ladder::moog_ladder_904a",
    ),
    // The gates frozen at 3.4 V.
    classic(
        "mxr_phase90",
        || topologies::mxr_phase90(3.4),
        "stave_dsp::circuit::topologies::mxr_phase90::mxr_phase90",
    ),
    classic("boss_ds1", topologies::boss_ds1, "stave_dsp::circuit::topologies::boss_ds1::boss_ds1"),
    // Gain and treble at noon.
    classic(
        "klon_centaur",
        || topologies::klon_centaur(0.5, 0.5),
        "stave_dsp::circuit::topologies::klon_centaur::klon_centaur",
    ),
    // Distortion and output at maximum.
    classic(
        "mxr_distortion_plus",
        || topologies::mxr_distortion_plus(1.0, 1.0),
        "stave_dsp::circuit::topologies::mxr_distortion_plus::mxr_distortion_plus",
    ),
];

/// The circuit `name`.
pub fn find(name: &str) -> &'static Circuit {
    CIRCUITS
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no circuit {name}"))
}

/// `name`'s model at `tier`: `Circuit<Dk>` on the deck, or a classic's
/// catalogued topology on the deck's plan.
pub fn model(name: &str, tier: Tier) -> Characterise {
    let c = Characterise::ngspice(name).tier(tier);
    match find(name).classic {
        Some((topology, catalogue)) => c.netlist(topology()).catalogue(catalogue),
        None => c.circuit(),
    }
}

/// The solver's rate for a case at `tier`: the oversampling its waveform
/// case runs at (`matches_ngspice.rs`, `dk_matches_ngspice.rs`,
/// `transistors_match_ngspice.rs`) — 16× where an op-amp's feedback pole
/// is faster than a 4× step (8× for the Tube Screamer's T0, for time),
/// 1× for a memoryless circuit's T0 (no step to refine: its "oversampled"
/// run is its 48 kHz one; T1's 4× measures it free of aliasing), 16× for
/// the transistor decks whose cards' charge storage puts poles far above
/// the audio band (DSP.md R11), else 4×. The classics run at the grid
/// their waveform cases converge on (`classics_match_ngspice.rs`): 32×
/// where an op-amp slews or rails into hard clipping (the RAT's LM308,
/// the DS-1, the Distortion+'s 741), 16× for the other op-amp and triode
/// circuits and the Big Muff, 8× for the Klon and the resonant Moog
/// ladder.
pub fn oversample(case: &str, tier: Tier) -> u32 {
    match (case.split('/').next().unwrap(), tier) {
        ("proco_rat" | "boss_ds1" | "mxr_distortion_plus", _) => 32,
        ("tube_screamer_ts808" | "big_muff_rams_head" | "marshall_jcm800_2203" | "mxr_phase90", _) => 16,
        ("klon_centaur" | "moog_ladder_feedback", _) => 8,
        ("tube_screamer", Tier::T0) => 8,
        ("opamp_clipper" | "tube_screamer", _) => 16,
        ("fuzz_face" | "ce_stage" | "mosfet_booster", _) => 16,
        ("triode_stage" | "push_pull" | "push_pull_pentode", Tier::T0) => 1,
        _ => 4,
    }
}

/// How each circuit ships (DSP.md R12), chosen from `shipping_rate.rs`'s
/// reports: the cheapest of 1×–16× oversampling, with or without
/// stateful ADAA (A1), whose worst ANMR over the piano at +12 and +20 dB
/// re the knee at 44.1 kHz (the worst host rate) is inaudible, at most
/// −10 dB (Kahles et al. 2019), whose C2 response stays within 1 dB to
/// 16 kHz, and which holds the matrix's existing phase budgets. That
/// rules out the Fuzz Face at 1× (its AC128 transit time reads 5.7 dB
/// low at 16 kHz at 44.1 kHz: it ships at 4× for its response, not its
/// aliasing) and, for now, ADAA: it halves the factor a clipper needs
/// (the op-amp clipper reaches −17.2 dB at 4× with it, −33.7 at 8×
/// without) but delays by half an inner sample (0.95° at 1 kHz at 4×,
/// 17° at 16 kHz) and, at 2×, bends the response (designed for 1.5·T,
/// 4.4 dB low at 16 kHz on the Tube Screamer).
/// Three circuits read above −10 dB at every factor — the CE
/// stage (+10.0 dB at 8× and 16×), the JFET booster (−6.6 at 27 Hz) and
/// the MOSFET booster (+13.5): that floor does not fall with
/// oversampling, so it is not aliasing (R19's sweep, a finding); they
/// ship at the factor that reaches it. No circuit is prewarped: a
/// prewarp to 16 kHz moved every response's error below it by more than
/// it took away above (the RC clipper alone gained, 4.1 → 1.7 dB at
/// 48 kHz; 4× takes it to 0.006 dB). A hand topology ships as its circuit
/// does, oversampled only.
pub static SHIPPING: [(&str, Shipping); 16] = [
    ("diode_clipper", Shipping::oversampled(8)),
    ("opamp_clipper", Shipping::oversampled(8)),
    ("tube_screamer", Shipping::oversampled(4)),
    ("common_cathode", Shipping::PLAIN),
    ("common_cathode_grid_current", Shipping::oversampled(8)),
    ("triode_stage", Shipping::oversampled(4)),
    ("push_pull", Shipping::oversampled(8)),
    ("push_pull_pentode", Shipping::oversampled(8)),
    ("power_supply", Shipping::PLAIN),
    ("fuzz_face", Shipping::oversampled(4)),
    ("rangemaster", Shipping::PLAIN),
    ("ce_stage", Shipping::oversampled(8)),
    ("jfet_booster", Shipping::oversampled(8)),
    ("mosfet_booster", Shipping::oversampled(8)),
    ("ota_vca", Shipping::oversampled(2)),
    // Its sixth-order stopband, bent by the bilinear map, holds the
    // followers' folded products only from 16× (−20.5 dB; 8×: −3.1).
    ("boss_ce2_filters", Shipping::oversampled(16)),
];

/// `case`'s shipping configuration (a hand topology's is its circuit's,
/// without a prewarp: it has its own discretisation; a `_static` deck's
/// is its full card's).
pub fn shipping(case: &str) -> Shipping {
    let mut parts = case.split('/');
    let first = parts.next().unwrap();
    let circuit = first.strip_suffix("_static").unwrap_or(first);
    let s = SHIPPING.iter().find(|(n, _)| *n == circuit).map_or(Shipping::PLAIN, |s| s.1);
    if parts.next() == Some("hand") {
        Shipping::oversampled(s.oversample)
    } else {
        s
    }
}

/// Each case's budgets at `tier` (cases are `<circuit>` for
/// `Circuit<Dk>` and `<circuit>/<topology>` for a hand topology).
pub fn budgets(case: &str, tier: Tier, c: Characterise) -> Characterise {
    let mut c = c.oversampled(oversample(case, tier)).shipped(shipping(case));
    for &(check, rates, first, second) in budgets::table(case, tier) {
        let b = if second.is_nan() {
            Budget::db(first)
        } else {
            Budget::db(first).degrees(second)
        };
        c = match rates {
            Rates::Oversampled => c.budget(check, b),
            Rates::Shipping => c.shipping(check, b),
        };
    }
    c
}

mod budgets;

/// The threads a sweep may use: `RAYON_NUM_THREADS` (which
/// `scripts/offload.sh` sets to the slot's share of the cores), else the
/// machine's parallelism.
pub fn threads() -> usize {
    std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .filter(|&n: &usize| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// `f` over `items` on at most [`threads`] threads, results in order.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads().min(items.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let r = f(item);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(Option::unwrap).collect()
}

/// `Circuit<Dk>` on `name`'s deck as it ships at the host rate `host`:
/// prepared at `host` times its oversampling, prewarped and anti-aliased
/// as `s` says.
pub fn shipped_circuit(
    deck: &stave_dsp::circuit::netlist::Netlist,
    input: &Input,
    host: f64,
    s: Shipping,
) -> stave_dsp::circuit::Circuit<stave_dsp::circuit::methods::dk::Dk> {
    let mut c = stave_test::characterise::model::circuit(deck, input, host * f64::from(s.oversample));
    if s.prewarp.is_some() {
        c.set_prewarp(s.prewarp);
    }
    if s.adaa {
        c.set_antialiasing(stave_dsp::circuit::methods::adaa::Antialiasing::Adaa);
    }
    c
}

/// One whole period at `m` times the host rate back to the host rate by
/// an ideal decimator: every component below the host's Nyquist kept,
/// everything above it gone (a real decimator's own passband and
/// stopband are `oversampler_conformance.rs`'s).
pub fn decimate(y: &[f64], m: usize) -> Vec<f64> {
    if m == 1 {
        return y.to_vec();
    }
    let n = y.len();
    let (mut re, mut im) = (y.to_vec(), vec![0.0; n]);
    stave_measure::spectrum::fft(&mut re, &mut im);
    let h = n / m;
    // Keep bins 0..h/2 and their mirror images, scaled to the shorter
    // period; the inverse transform as the forward one of the conjugate.
    let (mut r2, mut i2) = (vec![0.0; h], vec![0.0; h]);
    for k in 0..h / 2 {
        r2[k] = re[k];
        i2[k] = -im[k];
        if k > 0 {
            r2[h - k] = re[n - k];
            i2[h - k] = -im[n - k];
        }
    }
    stave_measure::spectrum::fft(&mut r2, &mut i2);
    r2.iter().map(|x| x / n as f64).collect()
}

/// The worst ANMR over the piano (every semitone, 27.5 Hz to 4186 Hz) at
/// `offset` dB re `name`'s knee, at the host rate `host` as the circuit
/// ships in `s`: `(ANMR dB, at Hz)`.
pub fn piano_anmr(name: &str, host: f64, s: Shipping, offset: f64) -> (f64, f64) {
    use stave_measure::aliasing::{anmr, SteppedTone};
    use stave_test::characterise::{deck, Plan};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{name}.plan"))).unwrap()).unwrap();
    // A classic runs as its catalogued topology.
    let deck = find(name).classic.map_or_else(|| deck(name), |(topology, _)| topology());
    let sweep = SteppedTone {
        amplitude: plan.knee.unwrap().at(offset),
        // Past the slowest settling (≤ 50 ms) in one window.
        warmup_windows: 1,
        ..SteppedTone::piano(host, 88)
    };
    let m = s.oversample as usize;
    let base = shipped_circuit(&deck, &plan.input, host, s);
    sweep
        .bins()
        .into_iter()
        .map(|bin| {
            let mut c = base.clone();
            // The same tone (bin `bin` of the window) at m× the rate.
            let n = sweep.window * m;
            let total = (sweep.warmup_windows + 1) * n;
            let mut out = Vec::with_capacity(n);
            for i in 0..total {
                let x = sweep.amplitude * (std::f64::consts::TAU * (bin * (i % n)) as f64 / n as f64).sin();
                let y = if i == 0 { c.output() } else { c.tick_f64(x) };
                if i >= total - n {
                    out.push(y);
                }
            }
            (anmr(&decimate(&out, m), bin, host), bin as f64 * host / sweep.window as f64)
        })
        .fold((f64::NEG_INFINITY, 0.0), |a, b| if b.0 > a.0 { b } else { a })
}

/// A fresh hand stage at a rate.
type Make = Box<dyn Fn(f64) -> Box<dyn FnMut(f64) -> f64>>;

/// A hand topology's case (`<circuit>/<topology>`) at `tier`, run and
/// asserted: each ships as its own type, so each is held to ngspice on
/// its circuit's plan directly.
pub fn hand(case: &str, tier: Tier) {
    use stave_dsp::{
        CommonCathode, CommonCathodeConfig, Dempwolf, DiodeClipper, DiodeClipperConfig, Koren,
        OpAmpClipper, OpAmpClipperConfig, PairSolver, PowerSupply, PowerSupplyConfig, Shockley,
        Stage,
    };
    fn cathode<T: stave_dsp::circuit::devices::triode::Triode + Copy + 'static>(
        tube: T,
        source_ohms: f64,
    ) -> (Make, f64) {
        let make = move |rate: f64| {
            let mut c = CommonCathode::new(CommonCathodeConfig {
                tube,
                source_ohms,
                supply_volts: 300.0,
                plate_load_ohms: 100e3,
                cathode_ohms: 1.5e3,
                bypass_farads: 22e-6,
                newton: Default::default(),
            });
            c.prepare(rate as f32);
            c
        };
        let plate = make(48_000.0).plate_volts();
        let stage: Make = Box::new(move |rate| {
            let mut c = make(rate);
            Box::new(move |x| c.tick_f64(x))
        });
        (stage, plate)
    }
    let circuit = case.split('/').next().unwrap();
    let catalogue = match case {
        "diode_clipper/hand" => "stave_dsp::circuit::topologies::diode_clipper::DiodeClipper",
        "opamp_clipper/hand" => "stave_dsp::circuit::topologies::opamp_clipper::OpAmpClipper",
        "common_cathode/hand" | "common_cathode_grid_current/hand" => "stave_dsp::circuit::topologies::common_cathode::CommonCathode",
        "power_supply/hand" => "stave_dsp::circuit::topologies::power_supply::PowerSupply",
        other => panic!("no hand topology {other}"),
    };
    let base = Characterise::ngspice(circuit).named(case).tier(tier).catalogue(catalogue);
    let c = match case {
        "diode_clipper/hand" => base
            .stage(|rate| {
                let mut c = DiodeClipper::new(DiodeClipperConfig {
                    input_ohms: 2.2e3,
                    capacitor_farads: 10e-9,
                    diode: Shockley::N4148,
                    solver: PairSolver::default(),
                });
                c.prepare(rate as f32);
                move |x| f64::from(c.tick(x as f32))
            })
            .operating_point("out", 0.0),
        "opamp_clipper/hand" => base
            .stage(|rate| {
                let mut c = OpAmpClipper::new(OpAmpClipperConfig::TS808);
                c.prepare(rate as f32);
                move |x| c.tick_f64(x)
            })
            .operating_point("out", 0.0),
        "common_cathode/hand" => {
            let (stage, plate) = cathode(Koren::T12AX7, 0.0);
            base.stage(stage).operating_point("p", plate)
        }
        "common_cathode_grid_current/hand" => {
            let (stage, plate) = cathode(Dempwolf::RSD_1, 33e3);
            base.stage(stage).operating_point("p", plate)
        }
        // The supply ticks to the end of each step with that step's load
        // (the plan's current rides on the deck's 20 mA): sample 0 is the
        // switch-on state.
        "power_supply/hand" => base.stage(|rate| {
            let mut s = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
            s.prepare(rate as f32);
            let mut first = true;
            move |x| {
                if std::mem::take(&mut first) {
                    0.0
                } else {
                    s.tick_f64(0.02 + x)
                }
            }
        }),
        other => panic!("no hand topology {other}"),
    };
    // Each hand topology pins its fingerprint (DSP.md R18) at T0.
    budgets(case, tier, c).run().pinned().assert();
}
