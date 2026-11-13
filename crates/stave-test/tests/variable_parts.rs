//! N6: a running circuit's knobs turned, held to ngspice at every pot
//! position and to the ear's threshold for clicks.
//!
//! - **Tracking.** One circuit is built at one setting and turned to the
//!   others at run time (a low-rank update, never a rebuild); at each it
//!   meets the budget the deck written at that setting meets: the
//!   Yeh–Smith tone stack's three AC decks (`tone_stack_duncan_fender_*`: treble, bass and mid pots)
//!   and the Tube Screamer's drive pot at 0.1, 0.5 and 0.9
//!   (`tube_screamer_drive_*.cir`, the stage's transient at 16×), each
//!   through `Circuit<Dk>` and `Circuit<Wdf>`.
//! - **Clicks.** A 1 kHz tone through the tone stack while block-rate
//!   automation sweeps the treble pot end to end: the energy off the
//!   carrier and its legitimate ramp skirt (stave-measure's zipper
//!   spray) with the default smoothing must sit far below audibility,
//!   and far below the same sweep applied in raw block steps — so the
//!   measure can see what it is there to catch.

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
use stave_dsp::circuit::{Circuit, Method, Pot, Smoothing};
use stave_measure::zipper::{zipper_report, ZipperConfig};
use stave_test::reference::{Case, Metric, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The tone stack's `mid` deck with its pots declared: treble and mid
/// (the leg to the wiper's lower side grows), bass a rheostat on a 1 M
/// track. A wave digital filter wants an end resistance: 1 mΩ.
fn tone_stack(end_ohms: f64) -> Netlist {
    let mut n = deck("tone_stack_duncan_fender_mid");
    let pot = |name, legs| Variable::pot(name, legs, Taper::Linear).with_end_ohms(end_ohms);
    n.add_variable(pot("treble", ["rt2", "rt1"])).unwrap();
    n.add_variable(pot("mid", ["rm2", "rm1"])).unwrap();
    n.add_variable(Variable::rheostat("rb", 1e6 - end_ohms, Taper::Linear).with_end_ohms(end_ohms))
        .unwrap();
    n
}

/// The settings the tone stack decks were written at: (treble, bass, mid).
const TONE_STACK_DECKS: [(&str, [f64; 3]); 3] = [
    ("tone_stack_duncan_fender_bright", [0.9, 0.1, 0.7]),
    ("tone_stack_duncan_fender_dark", [0.2, 0.8, 0.3]),
    ("tone_stack_duncan_fender_mid", [0.5, 0.5, 0.5]),
];

/// What the two methods share here.
trait Knobs {
    fn knob(&self, name: &str) -> Pot;
    fn turn(&mut self, pot: Pot, x: f64);
    fn settle_now(&mut self);
    fn smoothing(&mut self, s: Smoothing);
    fn response(&self, hz: f64) -> f64;
    fn drive_source(&mut self, name: &str);
    fn volts(&self) -> f64;
}

macro_rules! knobs {
    ($m:ty) => {
        impl Knobs for Circuit<$m> {
            fn knob(&self, name: &str) -> Pot {
                self.pot(name).unwrap()
            }
            fn turn(&mut self, pot: Pot, x: f64) {
                self.set_position(pot, x);
            }
            fn settle_now(&mut self) {
                self.settle();
            }
            fn smoothing(&mut self, s: Smoothing) {
                self.set_smoothing(s);
            }
            fn response(&self, hz: f64) -> f64 {
                self.analog_response(hz).unwrap().magnitude()
            }
            fn drive_source(&mut self, name: &str) {
                self.drive(name).unwrap();
            }
            fn volts(&self) -> f64 {
                self.output()
            }
        }
    };
}
knobs!(Dk);
knobs!(Wdf);

fn tone_stack_tracks<M: Method>(label: &str, end_ohms: f64, budget_db: f64)
where
    Circuit<M>: Knobs,
{
    let mut circuit = Circuit::<M>::new(&tone_stack(end_ohms)).unwrap();
    let [treble, bass, mid] = ["treble", "rb", "mid"].map(|n| circuit.knob(n));
    for (name, [t, b, m]) in TONE_STACK_DECKS {
        circuit.turn(treble, t);
        circuit.turn(bass, b);
        circuit.turn(mid, m);
        circuit.settle_now();
        Case::ngspice(name)
            .named(format!("{name}/{label}, turned from mid"))
            .budget(Metric::MaxDb, budget_db)
            .pointwise(|hz| circuit.response(hz))
            .assert();
    }
}

/// DK at the decks' own budget (1e-10 dB): with no end resistance the
/// turned pots are the decks' resistors exactly.
#[test]
fn tone_stack_pots_track_ngspice_on_dk() {
    tone_stack_tracks::<Dk>("dk", 0.0, 1e-10);
}

/// WDF with 1 mΩ of end resistance (which the decks leave out: up to
/// 1e-8 of a leg, so the budget is 1e-6 dB).
#[test]
fn tone_stack_pots_track_ngspice_on_wdf() {
    tone_stack_tracks::<Wdf>("wdf", 1e-3, 1e-6);
}

/// The drive deck at 0.5 with its pot declared as the decks compute it.
fn screamer() -> Netlist {
    let mut n = deck("tube_screamer_drive_50");
    n.add_variable(Variable::rheostat("rd", 500e3, Taper::AUDIO).with_end_ohms(50.0))
        .unwrap();
    n
}

/// One circuit at drive 0.5, turned to each deck's position, reset to
/// its operating point (which the drive does not move: no DC flows in
/// the feedback) and run on the deck's stimulus at 16×. Budget: the
/// Tube Screamer deck's (6e-5 normalized RMS).
fn screamer_tracks<M: Method>(label: &str)
where
    Circuit<M>: Knobs,
    Circuit<M>: stave_dsp::Stage,
{
    let rate = 48_000.0 * 16.0;
    let mut circuit = Circuit::<M>::new(&screamer()).unwrap();
    stave_dsp::Stage::prepare(&mut circuit, rate as f32);
    let drive = circuit.knob("rd");
    for (name, position) in [("tube_screamer_drive_10", 0.1), ("tube_screamer_drive_90", 0.9), ("tube_screamer_drive_50", 0.5)] {
        circuit.turn(drive, position);
        circuit.settle_now();
        stave_dsp::Stage::reset(&mut circuit);
        let reference = Reference::ngspice(name);
        let mut n = 0u64;
        let mut out = circuit.volts();
        let run: Vec<f64> = reference
            .abscissa()
            .iter()
            .map(|&t| {
                while n < (t * rate).round() as u64 {
                    out = circuit.tick_f64(0.0);
                    n += 1;
                }
                out
            })
            .collect();
        Case::ngspice(name)
            .named(format!("{name}/{label} 16x, turned from 0.5"))
            .budget(Metric::NormalizedRms, 6e-5)
            .compare(&run)
            .assert();
    }
}

#[test]
fn tube_screamer_drive_tracks_ngspice_on_dk() {
    screamer_tracks::<Dk>("dk");
}

#[test]
fn tube_screamer_drive_tracks_ngspice_on_wdf() {
    screamer_tracks::<Wdf>("wdf");
}

/// The zipper spray of a 1 kHz tone through the tone stack while the
/// treble pot sweeps 0 → 1 in `block`-sample automation steps.
fn sweep_spray<M: Method>(smoothing: Smoothing, block: usize) -> (f64, f64)
where
    Circuit<M>: Knobs + stave_dsp::Stage,
{
    const RATE: f64 = 48_000.0;
    const N: usize = 1 << 15;
    let carrier_bin = 683; // 1000.5 Hz
    let mut circuit = Circuit::<M>::new(&tone_stack(1e-3)).unwrap();
    circuit.drive_source("vi");
    stave_dsp::Stage::prepare(&mut circuit, RATE as f32);
    circuit.smoothing(smoothing);
    let treble = circuit.knob("treble");
    circuit.turn(treble, 0.0);
    circuit.settle_now();
    let blocks = N / block;
    let mut out = Vec::with_capacity(N);
    for n in 0..N {
        if n % block == 0 {
            circuit.turn(treble, (n / block) as f64 / (blocks - 1) as f64);
        }
        let x = (std::f64::consts::TAU * carrier_bin as f64 * n as f64 / N as f64).sin();
        out.push(circuit.tick_f64(x));
    }
    let r = zipper_report(
        &out,
        &ZipperConfig {
            carrier_bin,
            sample_rate: RATE,
            guard_hz: 100.0,
            low_cut_hz: 20.0,
        },
    );
    (r.spray_db, r.worst.1)
}

/// Default smoothing keeps a full sweep's spray below −100 dB under
/// 64-sample automation (stepped: −62) and below −70 under 512-sample
/// automation (stepped: −48; a 5 ms glide cannot hide a 10.7 ms
/// staircase entirely — 10 and 20 ms reach −82 and −94), on both
/// methods; refresh intervals and glide times are printed for the
/// record.
#[test]
fn a_pot_sweep_under_a_tone_does_not_click() {
    for interval in [1, 2, 4, 8, 16, 64] {
        let s = Smoothing {
            interval,
            ..Smoothing::DEFAULT
        };
        let (spray, worst) = sweep_spray::<Dk>(s, 64);
        println!("  dk, 64-sample automation, refresh every {interval:>2}: spray {spray:.1} dB (worst bin {worst:.1})");
    }
    for time in [5.0, 10.0, 20.0] {
        let s = Smoothing {
            time: stave_dsp::units::ms(time),
            ..Smoothing::DEFAULT
        };
        let (spray, worst) = sweep_spray::<Dk>(s, 512);
        println!("  dk, 512-sample automation, {time} ms glide: spray {spray:.1} dB (worst bin {worst:.1})");
    }
    for (block, threshold) in [(64, -100.0), (512, -70.0)] {
        for (label, spray) in [
            ("dk", sweep_spray::<Dk> as fn(Smoothing, usize) -> (f64, f64)),
            ("wdf", sweep_spray::<Wdf>),
        ] {
            let (stepped, stepped_worst) = spray(Smoothing::NONE, block);
            let (smooth, smooth_worst) = spray(Smoothing::DEFAULT, block);
            println!(
                "  {label}, {block}-sample automation: stepped spray {stepped:.1} dB (worst bin {stepped_worst:.1}), \
                 default smoothing {smooth:.1} dB (worst bin {smooth_worst:.1})"
            );
            assert!(smooth < threshold, "{label} {block}: {smooth} dB");
            assert!(stepped > smooth + 20.0, "{label} {block}: the measure sees steps ({stepped} vs {smooth})");
        }
    }
}
