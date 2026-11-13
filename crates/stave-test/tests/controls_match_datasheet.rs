//! H6: controls as hardware. Tapers against the manufacturer's curves
//! (Alpha's, as Holmes & van Walstijn fitted them, DAFx-19 Table 2) and
//! against real pots the paper's authors measured; dual-gang tracking
//! as a tolerance model; a scratchy wiper; switches that pop, bounce and
//! (with a ramp) do not click.

use stave_dsp::circuit::devices::potentiometer::{Bounce, LinearCubic, Tracking, Wiper, MAX_BOUNCES};
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
use stave_dsp::circuit::{Circuit, Smoothing};
use stave_dsp::Stage;

const ALPHA_A: [(LinearCubic, f64); 6] = [
    (LinearCubic::ALPHA_05A, 0.05),
    (LinearCubic::ALPHA_10A, 0.10),
    (LinearCubic::ALPHA_15A, 0.15),
    (LinearCubic::ALPHA_20A, 0.20),
    (LinearCubic::ALPHA_25A, 0.25),
    (LinearCubic::ALPHA_30A, 0.30),
];

#[test]
fn alpha_tapers_hold_their_codes() {
    let mut report = Vec::new();
    let mut dips = Vec::new();
    for (law, code) in ALPHA_A {
        let taper = Taper::Curve(law);
        // Through the paper's points exactly.
        for &[x, y] in law.points() {
            assert!((taper.fraction(x) - y).abs() < 1e-12);
        }
        // An A taper is coded by its fraction at half rotation.
        let half = taper.fraction(0.5);
        report.push((code, half));
        assert!((half - code).abs() < 0.008, "{code}: {half}");
        // Monotone but for the paper's cubics' small dips where a short
        // run meets a steep one (under 3e-4 of the track), from 0 to 1.
        let mut last = 0.0;
        let mut dip: f64 = 0.0;
        for i in 0..=10_000 {
            let f = taper.fraction(f64::from(i) / 10_000.0);
            dip = dip.max(last - f);
            last = last.max(f);
        }
        assert!(dip < 3e-4, "{code}: dips {dip}");
        dips.push(dip);
        assert_eq!(taper.fraction(0.0), 0.0);
        assert!((taper.fraction(1.0) - 1.0).abs() < 1e-12);
        // The reverse is the mirror.
        let c = taper.reversed();
        for i in 0..=20 {
            let x = f64::from(i) / 20.0;
            assert!((c.fraction(x) - (1.0 - taper.fraction(1.0 - x))).abs() < 1e-12);
        }
    }
    eprintln!("(code, fraction at half rotation): {report:.4?}; dips {dips:?}");
}

/// Three Alpha 100 kΩ log (A) and three linear (B) pots measured every
/// 15° over their 300° travel, terminal 1 to wiper over the whole track:
/// Holmes & van Walstijn's companion data to DAFx-19 (§2.3,
/// `alpha_pots_csv.zip`), R/R_T at x = 0, 0.05, …, 1.
const MEASURED_LOG: [(f64, [f64; 21]); 3] = [
    (98_140.0, [1.207, 233.5, 993.7, 1685.9, 2395.0, 5129.0, 7681.0, 9786.0, 11667.0, 14119.0, 16500.0, 18798.0, 20880.0, 33850.0, 47180.0, 62090.0, 78360.0, 92190.0, 97800.0, 98200.0, 98200.0]),
    (83_400.0, [1.095, 49.4, 434.6, 1071.9, 1738.4, 2782.0, 5332.0, 7134.0, 9418.0, 11622.0, 13501.0, 15604.0, 17917.0, 23460.0, 34670.0, 45530.0, 56360.0, 69560.0, 81510.0, 83380.0, 83410.0]),
    (100_420.0, [1.045, 49.35, 579.3, 1215.7, 1973.7, 3090.0, 6271.0, 8978.0, 11003.0, 13121.0, 15156.0, 17836.0, 20160.0, 29820.0, 44500.0, 58290.0, 73000.0, 89870.0, 99440.0, 100490.0, 100490.0]),
];
const MEASURED_LINEAR: [(f64, [f64; 21]); 3] = [
    (99_450.0, [1.069, 1537.6, 9888.0, 16309.0, 21970.0, 27490.0, 32990.0, 38010.0, 43820.0, 48850.0, 54040.0, 59180.0, 64350.0, 69890.0, 74810.0, 80010.0, 84910.0, 90960.0, 96730.0, 99280.0, 99290.0]),
    (93_640.0, [1.166, 28.43, 6218.0, 12255.0, 18957.0, 24160.0, 29080.0, 33770.0, 38640.0, 43810.0, 48360.0, 53560.0, 58090.0, 63710.0, 68620.0, 73660.0, 78950.0, 85130.0, 91660.0, 93640.0, 93670.0]),
    (95_040.0, [1.233, 1.382, 4655.0, 9901.0, 15108.0, 20960.0, 25680.0, 31440.0, 36740.0, 41720.0, 47140.0, 52320.0, 57650.0, 62730.0, 67690.0, 73360.0, 78840.0, 84520.0, 90070.0, 94920.0, 94940.0]),
];

/// The worst |measured − law| over the 21 points, as a fraction of the
/// track, and where.
fn worst(law: Taper, unit: &(f64, [f64; 21])) -> (f64, f64) {
    let (total, ohms) = unit;
    ohms.iter()
        .enumerate()
        .map(|(i, r)| {
            let x = i as f64 * 0.05;
            ((r / total - law.fraction(x)).abs(), x)
        })
        .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
}

#[test]
fn measured_alpha_pots_sit_on_their_curves() {
    // The paper finds its units near 15A and B but with the upper knee
    // earlier (its Table 3): the laws are the sheet's, the units vary.
    // Reported; the budget bounds a unit's departure from its sheet.
    let mut report = Vec::new();
    for unit in &MEASURED_LOG {
        let (err, x) = worst(Taper::Curve(LinearCubic::ALPHA_15A), unit);
        report.push(("15A", err, x));
        assert!(err < 0.2, "log unit: {err} at {x}");
    }
    for unit in &MEASURED_LINEAR {
        let (err, x) = worst(Taper::Linear, unit);
        report.push(("B", err, x));
        assert!(err < 0.12, "linear unit: {err} at {x}");
    }
    eprintln!("(law, worst error of track, at rotation): {report:.3?}");
}

#[test]
fn gangs_track_within_their_spec() {
    let (spec, reference) = (3.0, 40.0);
    let fractions: Vec<f64> = (0..=40).map(|i| 10f64.powf(-f64::from(i) / 20.0)).collect();
    let mut errors = Vec::new();
    for seed in 0..10_000u64 {
        let gang = Tracking::draw(spec, reference, seed);
        assert_eq!(gang, Tracking::draw(spec, reference, seed));
        for &f in &fractions {
            let db = 20.0 * (gang.fraction(f) / f).log10();
            assert!(db.abs() <= spec + 1e-9, "seed {seed} at {f}: {db} dB");
        }
        errors.push(gang.error_db);
    }
    // Uniform over ±spec: mean |e| = spec/2, a tenth beyond 0.9·spec.
    let mean = errors.iter().map(|e| e.abs()).sum::<f64>() / errors.len() as f64;
    let tail = errors.iter().filter(|e| e.abs() > 0.9 * spec).count() as f64 / errors.len() as f64;
    assert!((mean - 1.5).abs() < 0.04, "{mean}");
    assert!((tail - 0.1).abs() < 0.015, "{tail}");
}

const DIVIDER: &str = "* a volume pot, unloaded
V1 in 0 0
RA in out 50k
RB out 0 50k
.control
wrdata out v(out)
.endc";

fn volume(variable: Variable) -> Circuit<Dk> {
    let mut netlist = Netlist::parse_spice(DIVIDER).unwrap();
    netlist.add_variable(variable).unwrap();
    let mut dk = Circuit::<Dk>::new(&netlist).unwrap();
    dk.drive("v1").unwrap();
    dk.prepare(48_000.0);
    dk
}

#[test]
fn a_dual_gang_volume_mismatches_its_channels() {
    let law = Taper::Curve(LinearCubic::ALPHA_15A);
    let gang = Tracking::draw(3.0, 40.0, 11);
    let mut left = volume(Variable::pot("vol", ["rb", "ra"], law));
    let mut right = volume(Variable::pot("vol", ["rb", "ra"], law).with_tracking(gang));
    for position in [0.9, 0.6, 0.3, 0.15] {
        for c in [&mut left, &mut right] {
            let pot = c.pot("vol").unwrap();
            c.set_position(pot, position);
            c.settle();
        }
        let l = left.analog_response(1_000.0).unwrap();
        let r = right.analog_response(1_000.0).unwrap();
        let measured = 20.0 * (r.re.hypot(r.im) / l.re.hypot(l.im)).log10();
        let f = law.fraction(position);
        let expect = 20.0 * (gang.fraction(f) / f).log10();
        assert!((measured - expect).abs() < 1e-6, "{position}: {measured} dB against {expect}");
    }
}

#[test]
fn a_scratchy_wiper_crackles_only_while_turning() {
    // The knob swept at `speed` rotations a second, set every sample:
    // the scratchy pot's output less the clean one's is the jitter, of
    // rms scratch × speed; once still, the two are the same.
    let scratch = 0.01;
    let mut rms_at = Vec::new();
    for speed in [0.25, 1.0] {
        let wiper = Wiper {
            scratch,
            corner_hz: 8_000.0,
            seed: 5,
        };
        let mut clean = volume(Variable::pot("vol", ["rb", "ra"], Taper::Linear));
        let mut rough = volume(Variable::pot("vol", ["rb", "ra"], Taper::Linear).with_wiper(wiper));
        for c in [&mut clean, &mut rough] {
            c.set_smoothing(Smoothing::NONE);
        }
        let (pc, pr) = (clean.pot("vol").unwrap(), rough.pot("vol").unwrap());
        let samples = (0.5 / speed * 48_000.0) as usize;
        let mut sum = 0.0;
        for n in 0..samples {
            let x = 0.25 + speed * n as f64 / 48_000.0;
            clean.set_position(pc, x);
            rough.set_position(pr, x);
            let d = rough.tick_f64(1.0) - clean.tick_f64(1.0);
            if n > 0 {
                sum += d * d;
            }
        }
        let rms = (sum / (samples - 1) as f64).sqrt();
        rms_at.push(rms);
        assert!((rms / (scratch * speed) - 1.0).abs() < 0.02, "{speed}/s: {rms}");
        // Still: identical, sample for sample.
        for _ in 0..4 {
            rough.tick_f64(1.0);
            clean.tick_f64(1.0);
        }
        for _ in 0..480 {
            assert_eq!(rough.tick_f64(1.0), clean.tick_f64(1.0));
        }
    }
    eprintln!("jitter rms at 0.25 and 1 rotation/s: {rms_at:.5?}");
}

/// A switch after a coupling capacitor whose leakage biases it: closing
/// it pops by the DC the leak left across it.
const POP: &str = "* a leaky coupling cap into a switch
V1 bias 0 4.5
RB bias b 10k
C1 b a 1u
RL1 b a 10meg
RS a out 100meg
RL out 0 10k
.control
wrdata out v(out)
.endc";

fn popper(bleeder: bool, switch: Variable) -> Circuit<Dk> {
    let deck = if bleeder {
        POP.replace("RL out 0 10k", "RL out 0 10k\nRP a 0 1meg")
    } else {
        POP.to_string()
    };
    let mut netlist = Netlist::parse_spice(&deck).unwrap();
    netlist.add_variable(switch).unwrap();
    let mut dk = Circuit::<Dk>::new(&netlist).unwrap();
    dk.prepare(48_000.0);
    dk
}

/// The step at the output the instant the switch closes: the capacitor
/// holds `V_b − V_a` from the open circuit's DC, and the loop through
/// RB, the switch and RL (with RP across a) sets the current.
fn predicted_pop(bleeder: bool) -> f64 {
    let (rb, leak, open, rl, rp, on) = (10e3, 10e6, 100e6, 10e3, 1e6, 0.1);
    // Open: a sees `leak` up from b and `open + rl` (∥ rp) down to ground.
    let down = if bleeder { 1.0 / (1.0 / (open + rl) + 1.0 / rp) } else { open + rl };
    let i = 4.5 / (rb + leak + down);
    let (vb, va) = (4.5 - rb * i, down * i);
    let vc = vb - va;
    // Closed, at t = 0+: bias → RB → b; a = b − vc; a → on → out → RL,
    // a → RP (bleeder), b → leak → a (negligible, but kept).
    // Nodal at b and out with a = b − vc: solve the 2×2.
    let gp = if bleeder { 1.0 / rp } else { 0.0 };
    // KCL at b (b and a one supernode through the capacitor):
    // (vb' − 4.5)/rb + (va' − out)/on + va'·gp = 0, va' = vb' − vc;
    // KCL at out: (out − va')/on + out/rl = 0.
    let g_on = 1.0 / on;
    // out = va'·g_on/(g_on + 1/rl)
    let k = g_on / (g_on + 1.0 / rl);
    // (vb' − 4.5)/rb + (vb' − vc)(g_on(1 − k) + gp) = 0
    let g = g_on * (1.0 - k) + gp;
    let vb_new = (4.5 / rb + vc * g) / (1.0 / rb + g);
    (vb_new - vc) * k
}

#[test]
fn switches_pop_bounce_and_soften() {
    let hard = || Variable::switch("rs", 0.1, 100e6);
    let mut pops = Vec::new();
    for bleeder in [false, true] {
        let mut c = popper(bleeder, hard());
        let s = c.switch("rs").unwrap();
        let before = c.tick_f64(0.0);
        c.set_closed(s, true);
        let after = c.tick_f64(0.0);
        let expect = predicted_pop(bleeder);
        pops.push((after - before, expect));
        assert!(((after - before) / expect - 1.0).abs() < 0.005, "bleeder {bleeder}: {} against {expect}", after - before);
    }
    let reduction = 20.0 * (pops[1].0 / pops[0].0).log10();
    assert!(reduction < -19.0, "a 1 MΩ bleeder takes {reduction} dB off the pop");

    // Click suppression: the same throw over 10 ms. The click is the
    // output's first difference (its high-frequency content).
    let click = |switch: Variable| {
        let mut c = popper(false, switch);
        let s = c.switch("rs").unwrap();
        let mut last = c.tick_f64(0.0);
        c.set_closed(s, true);
        (0..4_800)
            .map(|_| {
                let y = c.tick_f64(0.0);
                let d = y - last;
                last = y;
                d * d
            })
            .sum::<f64>()
    };
    let soft = 10.0 * (click(hard().with_ramp(0.010)) / click(hard())).log10();
    assert!(soft < -18.0, "a 10 ms ramp: {soft} dB of click");

    // Bounce: as many edges as drawn, then closed for good.
    let bounce = Bounce {
        seconds: 0.003,
        count: 4,
        seed: 21,
    };
    let mut edges = [0u32; 2 * MAX_BOUNCES];
    let n = bounce.edges(48_000.0, 0, &mut edges);
    let mut c = popper(false, hard().with_bounce(bounce));
    let s = c.switch("rs").unwrap();
    let mut last = c.tick_f64(0.0);
    c.set_closed(s, true);
    let mut jumps = Vec::new();
    for t in 0..960u32 {
        let y = c.tick_f64(0.0);
        if (y - last).abs() > 0.5 {
            jumps.push(t);
        }
        last = y;
    }
    assert_eq!(jumps.len(), n + 1, "{jumps:?} against {:?}", &edges[..n]);
    assert!(*jumps.last().unwrap() <= 144, "{jumps:?}");
    eprintln!(
        "pops {pops:.4?} V (measured, predicted); bleeder {reduction:.1} dB; 10 ms ramp {soft:.1} dB; bounce edges {jumps:?}"
    );
}
