//! The primitive DATASHEET — ns/sample for every primitive, measured the
//! way a benchmark should (warm-up, repeated runs, median,
//! spread printed so a noisy run discredits itself). Like the
//! project perf reports it DOCUMENTS, never blocks: the numbers
//! belong in front of eyes when composing an engine ("can I afford
//! four of these per lane?"), not in a gate that cries wolf on
//! desktop hardware.
//!
//! Run: `cargo test --release -p stave-dsp --test datasheet -- --nocapture`
//! Debug builds skip (opt-level lies about release cost).
//!
//! Datasheet anchors (M4, apple-m1 target-cpu, 2026-07-06): most
//! primitives sit in the ones of ns/sample — Lfo 2.1, DelayLine
//! write+read_cubic 2.4, RandomWalk 2.6, Slew 2.9, Lag 4.1, Curve f_at
//! 4.4 (f_and_slope_at the same fetch), Allpass1 5.7, Biquad 7.1,
//! Adaa(1) 8.8, Hysteresis 10.5. The outlier is NonlinearRc's
//! Newton solve at ~63 — an implicit nonlinear ODE per sample is
//! the one primitive you budget around.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;
use stave_dsp::FilterSpec;
use stave_dsp::RcClipperConfig;
use stave_dsp::{AdaaOrder, AdsrConfig};
use std::hint::black_box;
use std::time::Instant;

use stave_dsp::primitives::spectral::{Complex, Head};
use stave_dsp::{
    Adaa, Adsr, Allpass1, Biquad, Convolver, Curve, DelayLine, DiodeClipper, DiodeClipperConfig,
    Fft, Hysteresis, ImpulseResponse, Lag, Lfo, NonlinearRc, Osc, PairSolver, RandomWalk, Shockley,
    Slew, Stage, Waveform,
};
use stave_dsp::{
    CommonCathode, CommonCathodeConfig, Koren, OpAmpClipper, OpAmpClipperConfig,
    PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents, Tube,
};

const N: usize = 1 << 17;
const REPEATS: usize = 5;

/// LCG noise in ±0.3 — the same stimulus family as the perf report.
fn noise(buf: &mut [f32]) {
    let mut lcg: u32 = 7;
    for x in buf.iter_mut() {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3;
    }
}

/// Median ns/sample over `REPEATS` timed passes (one warm-up pass),
/// printed with the spread.
fn bench(name: &str, input: &[f32], mut tick: impl FnMut(f32) -> f32) {
    let mut run = || {
        let t0 = Instant::now();
        let mut acc = 0.0f32;
        for &x in input {
            acc += tick(black_box(x));
        }
        black_box(acc);
        t0.elapsed().as_secs_f64() / input.len() as f64 * 1e9
    };
    run(); // warm
    let mut samples: Vec<f64> = (0..REPEATS).map(|_| run()).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[REPEATS / 2];
    let spread = (samples[REPEATS - 1] - samples[0]) / median * 100.0;
    println!("  {name:<28} {median:>7.2} ns/sample  (spread {spread:.0}%)");
}

/// Median ns per point of a whole-buffer operation over `REPEATS`
/// timed batches, for the transforms (cost per transform ÷ length).
fn bench_points(name: &str, points: usize, mut run_once: impl FnMut()) {
    let calls = (N / points).max(8);
    let mut run = || {
        let t0 = Instant::now();
        for _ in 0..calls {
            run_once();
        }
        t0.elapsed().as_secs_f64() / (calls * points) as f64 * 1e9
    };
    run();
    let mut samples: Vec<f64> = (0..REPEATS).map(|_| run()).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[REPEATS / 2];
    let spread = (samples[REPEATS - 1] - samples[0]) / median * 100.0;
    println!("  {name:<28} {median:>7.2} ns/sample  (spread {spread:.0}%)");
}

#[test]
fn primitive_datasheet() {
    if cfg!(debug_assertions) {
        eprintln!("datasheet SKIPPED (debug build — run under `cargo test --release`)");
        return;
    }
    let sr = 48_000.0;
    let mut input = vec![0.0f32; N];
    noise(&mut input);
    println!("stave-dsp primitive datasheet ({N} samples, median of {REPEATS})");

    let mut biquad = Biquad::new(FilterSpec::Lowpass {
        hz: 1_000.0,
        q: 0.707,
    });
    biquad.prepare(sr);
    bench("Biquad (lowpass)", &input, |x| biquad.tick(x));

    let mut ap = Allpass1::new(700.0);
    ap.prepare(sr);
    bench("Allpass1", &input, |x| ap.tick(x));

    let mut curve = Curve::flat();
    curve.rebuild(-4.0, 4.0, 0.0, |x| x.tanh());
    bench("Curve f_at", &input, |x| {
        curve.f_at(f64::from(x) * 3.0) as f32
    });
    bench("Curve f_and_slope_at", &input, |x| {
        curve.f_and_slope_at(f64::from(x) * 3.0).0 as f32
    });

    let mut adaa = Adaa::new(AdaaOrder::First);
    let framed = curve.framed(0.0, 1.0, 1.0);
    bench("Adaa(1) over Curve", &input, |x| {
        adaa.tick(&framed, f64::from(x) * 3.0) as f32
    });

    let mut hys = Hysteresis::new(2.2, 0.5);
    hys.prepare(sr);
    bench("Hysteresis", &input, |x| hys.tick(x));

    let mut slew = Slew::new(120.0, 120.0);
    slew.prepare(sr);
    bench("Slew", &input, |x| slew.tick(x));

    let mut rc = NonlinearRc::diode_clipper(&RcClipperConfig {
        tau_ms: 0.35,
        threshold: 0.5,
        hardness: 2.0,
    });
    rc.prepare(sr);
    bench("NonlinearRc (diode, Newton)", &input, |x| rc.tick(x));

    for (name, solver) in [
        ("DiodeClipper (WDF, Newton)", PairSolver::default()),
        ("DiodeClipper (WDF, Wright omega)", PairSolver::WrightOmega),
    ] {
        let mut wdf = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::D1N4148,
            solver,
        });
        wdf.prepare(sr);
        bench(name, &input, |x| wdf.tick(x));
    }

    circuit_datasheet(sr, &input);

    let mut line = DelayLine::new();
    line.allocate(4_800);
    bench("DelayLine write+read_cubic", &input, |x| {
        line.write(x);
        line.advance();
        line.read_cubic(1_234.6)
    });

    let mut lag = Lag::new(50.0, 50.0);
    lag.prepare(sr);
    bench("Lag", &input, |x| lag.tick(x));

    let mut lfo = Lfo::new(1.3);
    lfo.prepare(sr);
    bench("Lfo", &input, |x| lfo.tick(x));

    let mut wander = RandomWalk::new(140.0, 30.0, 7);
    wander.prepare(sr);
    bench("RandomWalk", &input, |x| wander.tick(x));

    let mut adsr = Adsr::new();
    adsr.prepare(sr);
    adsr.set(&AdsrConfig {
        attack_ms: 2.0,
        decay_ms: 50.0,
        sustain: 0.7,
        release_ms: 100.0,
    });
    adsr.gate_on();
    bench("Adsr", &input, |x| adsr.tick(x));

    let mut osc_sine = Osc::new(Waveform::Sine);
    osc_sine.prepare(sr);
    osc_sine.set_hz(220.0);
    bench("Osc (sine)", &input, |x| osc_sine.tick(x));

    let mut osc_saw = Osc::new(Waveform::Saw);
    osc_saw.prepare(sr);
    osc_saw.set_hz(220.0);
    bench("Osc (saw, polyBLEP)", &input, |x| osc_saw.tick(x));

    for n in [512usize, 4096] {
        let fft = Fft::<f32>::new(n);
        let mut data: Vec<Complex<f32>> =
            input[..n].iter().map(|&x| Complex::new(x, 0.0)).collect();
        bench_points(&format!("Fft f32 complex {n}"), n, || {
            fft.forward(black_box(&mut data))
        });
        let mut bins = vec![Complex::default(); fft.real_bins()];
        bench_points(&format!("Fft f32 real {n}"), n, || {
            fft.forward_real(black_box(&input[..n]), &mut bins)
        });
    }

    // Per sample of mono audio; IR lengths of a cabinet, a room and a
    // two-second hall at 48 kHz.
    for (taps, block, head) in [
        (1_024, 256, Head::Partitioned),
        (16_384, 256, Head::Partitioned),
        (96_000, 256, Head::Partitioned),
        (1_024, 64, Head::Direct),
        (16_384, 64, Head::Direct),
    ] {
        let ir = ImpulseResponse::mono(&input[..taps]);
        let mut conv = Convolver::uniform(&ir, block, head);
        conv.prepare(sr);
        let name = format!(
            "Convolver {taps} taps B={block}{}",
            if head == Head::Direct { " direct" } else { "" }
        );
        bench(&name, &input, |x| conv.tick(x));
    }

    // F3: the same lengths with a direct head and doubling partitions
    // up to 8192; the worst 64-sample block shows the spike where the
    // large partitions step.
    for (taps, block, largest) in [(16_384, 64, 4_096), (96_000, 64, 8_192)] {
        let ir = ImpulseResponse::mono(&input[..taps]);
        let mut conv = Convolver::non_uniform(&ir, block, largest, Head::Direct);
        conv.prepare(sr);
        let name = format!("Convolver {taps} taps B={block}..{largest} non-uniform");
        bench(&name, &input, |x| conv.tick(x));
        worst_block(&name, &input, 64, |b| conv.process(b));
    }
}

/// The slowest `block`-sample call of `process` over the input, after a
/// warm-up pass: the spike a host's deadline sees.
fn worst_block(name: &str, input: &[f32], block: usize, mut process: impl FnMut(&mut [f32])) {
    let mut buf = input.to_vec();
    for chunk in buf.chunks_mut(block) {
        process(chunk);
    }
    let mut buf = input.to_vec();
    let mut worst = 0.0f64;
    for chunk in buf.chunks_mut(block) {
        let t0 = Instant::now();
        process(black_box(chunk));
        worst = worst.max(t0.elapsed().as_secs_f64());
    }
    println!(
        "  {:<28} {:>7.2} ns/sample  (worst {block}-sample block)",
        format!("  {name}"),
        worst / block as f64 * 1e9
    );
}

/// A netlist as `Circuit<Dk>`, its `V1` driven by the input.
fn dk(deck: &str, sr: f32) -> Circuit<Dk> {
    circuit_from(Netlist::parse_spice(deck).unwrap(), sr)
}

fn circuit_from(netlist: Netlist, sr: f32) -> Circuit<Dk> {
    let mut c = Circuit::<Dk>::new(&netlist).unwrap();
    c.drive("v1").unwrap();
    c.prepare(sr);
    c
}

const CLIPPER: &str = "diode clipper
V1 in 0 0
R1 in out 2.2k
C1 out 0 10n
D1 out 0 DI
D2 0 out DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(out)
.endc";

const OPAMP_CLIPPER: &str = "TS808 clipper, ideal op-amp
V1 in 0 0
E1 inm 0 in 0 1
R1 inm s 4.7k
VS s g 0
C1 g 0 47n
E2 fb 0 in 0 1
F1 0 out VS 1
RF out fb 51k
CF out fb 51p
D1 out fb DI
D2 fb out DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(out)
.endc";

/// The common cathode with the Koren law as ngspice spells it.
const CATHODE_B: &str = "12AX7 common cathode
.param mu=100 ex=1.4 kg1=1060 kp=600 kvb=300
V1 g 0 0
VB b 0 300
RA b p 100k
RK k 0 1.5k
CK k 0 22u
BE e1 0 V = {V(p,k)/kp*ln(1+exp(kp*(1/mu+V(g,k)/sqrt(kvb+V(p,k)*V(p,k)))))}
RE e1 0 1G
BI p k I = {V(e1) > 0 ? 2*pow(V(e1),ex)/kg1 : 0}
.control
wrdata out v(p)
.endc";

const TONE_STACK: &str = "tone stack, pots at noon
V1 i 0 0
C1 i t 250p
RT1 t o 125000
RT2 o b 125000
R4 i n 100000
C2 n b 100n
RB b j 500000
RM1 j w 12500
RM2 w 0 12500
C3 n w 47n
.control
wrdata out v(o)
.endc";

/// `Circuit<Dk>` beside the hand-derived topology of the same netlist,
/// at the host rate, on the same ±0.3 V noise (scaled into each
/// circuit's working range).
fn circuit_datasheet(sr: f32, input: &[f32]) {
    println!("  circuits: hand-derived topology, then Circuit<Dk> on its netlist");

    let mut wdf = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::D1N4148,
        solver: PairSolver::default(),
    });
    wdf.prepare(sr);
    bench("DiodeClipper x10 (WDF)", input, |x| wdf.tick(10.0 * x));
    let mut clipper = dk(CLIPPER, sr);
    bench("Circuit<Dk> diode clipper x10", input, |x| clipper.tick(10.0 * x));

    let mut opamp = OpAmpClipper::new(OpAmpClipperConfig::TS808);
    opamp.prepare(sr);
    bench("OpAmpClipper", input, |x| opamp.tick(x));
    let mut opamp_dk = dk(OPAMP_CLIPPER, sr);
    bench("Circuit<Dk> op-amp clipper", input, |x| opamp_dk.tick(x));

    let mut cathode = CommonCathode::new(CommonCathodeConfig {
        tube: Koren::T12AX7,
        source_ohms: 0.0,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    cathode.prepare(sr);
    bench("CommonCathode x5 (Koren)", input, |x| cathode.tick(5.0 * x));
    let mut cathode_b = dk(CATHODE_B, sr);
    bench("Circuit<Dk> cathode x5 (B src)", input, |x| cathode_b.tick(5.0 * x));
    let mut cathode_t = circuit_from(cathode_netlist(), sr);
    bench("Circuit<Dk> cathode x5 (Koren)", input, |x| cathode_t.tick(5.0 * x));

    let mut stack = PassiveToneStack::new(PassiveToneStackComponents {
        treble_capacitance_farads: 250e-12,
        bass_capacitance_farads: 100e-9,
        midrange_capacitance_farads: 47e-9,
        treble_potentiometer_ohms: 250e3,
        bass_potentiometer_ohms: 1e6,
        midrange_potentiometer_ohms: 25e3,
        slope_resistance_ohms: 100e3,
    });
    stack.set_controls(PassiveToneControlPositions::new(0.5, 0.5, 0.5));
    stack.prepare(sr);
    bench("PassiveToneStack", input, |x| stack.tick(x));
    let mut stack_dk = dk(TONE_STACK, sr);
    bench("Circuit<Dk> tone stack", input, |x| stack_dk.tick(x));
}

/// The common cathode built in code, with stave's Koren device.
fn cathode_netlist() -> Netlist {
    let mut n = Netlist::new();
    let (g, b, p, k) = (n.add_node("g"), n.add_node("b"), n.add_node("p"), n.add_node("k"));
    let ground = stave_dsp::circuit::netlist::Node::GROUND;
    let parts = [
        ("v1", ElementKind::VoltageSource { a: g, b: ground, source: Source::dc(0.0) }),
        ("vb", ElementKind::VoltageSource { a: b, b: ground, source: Source::dc(300.0) }),
        ("ra", ElementKind::Resistor { a: b, b: p, ohms: 100e3 }),
        ("rk", ElementKind::Resistor { a: k, b: ground, ohms: 1.5e3 }),
        ("ck", ElementKind::Capacitor { a: k, b: ground, farads: 22e-6, initial_volts: None }),
        ("t1", ElementKind::Triode { plate: p, grid: g, cathode: k, tube: Tube::Koren(Koren::T12AX7) }),
    ];
    for (name, kind) in parts {
        n.add(name, kind).unwrap();
    }
    n.probe = Some(Probe::node(p));
    n
}
