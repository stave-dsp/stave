//! The primitive DATASHEET — ns/sample for every primitive, measured the
//! way a benchmark should (warm-up, repeated runs, median,
//! spread printed so a noisy run discredits itself). Like the
//! project perf reports it DOCUMENTS, never blocks: the numbers
//! belong in front of eyes when composing an engine ("can I afford
//! four of these per lane?"), not in a gate that cries wolf on
//! desktop hardware.
//!
//! Run: `cargo test --release -p stave-dsp --test datasheet -- --nocapture`
//!
//! The datasheet is data (DSP.md P1): `crates/stave-dsp/datasheet.json`
//! holds every row as measured on the reference machine it names, and
//! the docs site's Cost page is generated from it. Refresh it with
//! `scripts/datasheet.sh`. It cannot drift from this file: every run —
//! a debug one too, which times nothing (opt-level lies about release
//! cost) and runs each stage a few samples only to name it — fails when
//! a stage benched here is missing from the data or the data holds one
//! no longer benched. A new `bench(..)` line lands with refreshed data.
//! The machinery is `datasheet/sheet.rs`.
//!
//! The current numbers are the data's; as a shape (M4, apple-m1
//! target-cpu, 2026-07-06): most
//! primitives sit in the ones of ns/sample — Lfo 2.1, DelayLine
//! write+read_cubic 2.4, RandomWalk 2.6, Slew 2.9, Lag 4.1, Curve f_at
//! 4.4 (f_and_slope_at the same fetch), Allpass1 5.7, Biquad 7.1,
//! Adaa(1) 8.8, Hysteresis 10.5. The outlier is NonlinearRc's
//! Newton solve at ~63 — an implicit nonlinear ODE per sample is
//! the one primitive you budget around.
//!
//! The circuit section lists each hand-derived topology, then
//! `Circuit<Dk>` solving the same netlist, with its Newton iterations
//! per sample (mini, arm64, 2026-11-12): diode clipper 264 vs 553,
//! op-amp clipper 296 vs 646, common cathode 1040 vs 1959 (the deck's
//! B-source Koren law; 2062 with the Koren device), tone stack 9.7 vs
//! 29. The general method costs 2–3× the hand derivation; a topology
//! stays its own type where that matters. The transistor decks have no
//! hand topology (stave-mini, 2026-11-12): Fuzz Face 3500
//! (two Gummel–Poon AC128s, four controls, 7.4 iterations on ±0.3 V
//! noise), CE stage 784, JFET booster 573.
//!
//! Learned anchors (M3, 2026-11-12): WaveNet at Neural Amp Modeler's
//! standard size 2.2 µs/sample (12 + 6 channels 1.7), LSTM 1 x 16 0.32,
//! GRU 1 x 16 0.23 — a different scale: a capture network is the whole
//! amp, and costs like one.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;
use stave_dsp::learned::{nam, Gru, LoadError};
#[path = "datasheet/sheet.rs"]
mod sheet;

use sheet::{bench, bench_points, N, RATE};
use stave_dsp::FilterSpec;
use stave_dsp::RcClipperConfig;
use stave_dsp::{AdaaOrder, AdsrConfig};
use stave_dsp::effects::dynamics::{
    Compressor, GainComputer, Limiter, Peak, Peaks, ProgramDependent, Rms, Sidechain, Topology,
};
use stave_dsp::effects::reverb::{Dattorro, Fdn, Mixing, Schroeder};
use stave_dsp::primitives::time::delay::Delay;
use stave_dsp::primitives::time::interpolators::{
    Interpolator, Lagrange3, Lagrange5, Linear, Sinc, Thiran,
};
use std::hint::black_box;
use std::time::Instant;

use stave_dsp::primitives::spectral::{Complex, Fir, Head, Passband, Stft, Window};
use stave_dsp::units::SampleRate;
use stave_dsp::{
    Adaa, Adsr, Allpass1, Biquad, Convolver, Curve, DelayLine, DiodeClipper, DiodeClipperConfig,
    Fft, Hysteresis, ImpulseResponse, Lag, Lfo, NonlinearRc, Osc, PairSolver, RandomWalk, Shockley,
    Slew, Stage, Waveform,
};
use stave_dsp::{
    CommonCathode, CommonCathodeConfig, Koren, OpAmpClipper, OpAmpClipperConfig,
    PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents, Tube,
};

/// LCG noise in ±0.3 — the same stimulus family as the perf report.
fn noise(buf: &mut [f32]) {
    let mut lcg: u32 = 7;
    for x in buf.iter_mut() {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3;
    }
}

/// Weights in ±0.5 from an LCG, rounded so the JSON stays short.
fn weights(n: usize) -> String {
    let mut lcg: u32 = 11;
    let w: Vec<String> = (0..n)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            format!("{:.3}", (lcg >> 8) as f32 / (1u32 << 24) as f32 - 0.5)
        })
        .collect();
    format!("[{}]", w.join(","))
}

/// A network file with `{w}` standing for the weights, filled with as
/// many as the loader says the architecture needs.
fn network(template: &str) -> nam::Network {
    let expected = match nam::parse(&template.replace("{w}", "[]")) {
        Err(LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    };
    nam::parse(&template.replace("{w}", &weights(expected)))
        .unwrap()
        .network
}

/// A [`Delay`] whose time glides 10 → 15 ms and jumps back, so every
/// read is fractional.
fn modulated_delay<I: Interpolator>(name: &str, input: &[f32], sample_rate: f32) {
    let mut delay = Delay::<I>::new(20.0, 10.0);
    delay.prepare(sample_rate);
    let mut ms = 10.0f32;
    bench(name, input, |x| {
        ms = if ms > 15.0 { 10.0 } else { ms + 1.3e-4 };
        delay.set_ms(ms);
        delay.tick(x)
    });
}

#[test]
fn primitive_datasheet() {
    sheet::start();
    let sr = RATE;
    let mut input = vec![0.0f32; N];
    noise(&mut input);

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
            diode: Shockley::N4148,
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

    // S2: a 10 ms delay gliding over 5 ms (a chorus voice), per interpolator.
    modulated_delay::<Linear>("Delay<Linear> (modulated)", &input, sr);
    modulated_delay::<Lagrange3>("Delay<Lagrange3> (modulated)", &input, sr);
    modulated_delay::<Lagrange5>("Delay<Lagrange5> (modulated)", &input, sr);
    modulated_delay::<Thiran>("Delay<Thiran> (modulated)", &input, sr);
    modulated_delay::<Sinc<8>>("Delay<Sinc<8>> (modulated)", &input, sr);
    modulated_delay::<Sinc<16>>("Delay<Sinc<16>> (modulated)", &input, sr);

    // S3: reverbs, T60 2 s.
    for (name, mixing) in [
        ("Fdn (8 lines, Householder)", Mixing::Householder),
        ("Fdn (8 lines, Hadamard)", Mixing::Hadamard),
    ] {
        let mut fdn = Fdn::new(mixing);
        fdn.set_t60_s(2.0);
        fdn.prepare(sr);
        bench(name, &input, |x| fdn.tick(x));
    }
    let mut plate = Dattorro::new(50.0);
    plate.set_t60_s(2.0);
    plate.prepare(sr);
    bench("Dattorro (plate)", &input, |x| plate.tick(x));
    let mut schroeder = Schroeder::new();
    schroeder.set_t60_s(2.0);
    schroeder.prepare(sr);
    bench("Schroeder (4 combs, 2 allpasses)", &input, |x| schroeder.tick(x));
    let mut moorer = Schroeder::moorer();
    moorer.set_t60_s(2.0);
    moorer.prepare(sr);
    bench("Schroeder::moorer (6 combs)", &input, |x| moorer.tick(x));

    // D1–D3: dynamics.
    let mut peak = Peak::new(5.0, 50.0);
    peak.prepare(sr);
    bench("Peak detector", &input, |x| peak.tick(x));
    let mut rms = Rms::new(10.0);
    rms.prepare(sr);
    bench("Rms detector (10 ms)", &input, |x| rms.tick(x));
    let mut program = ProgramDependent::new(1.0, 50.0, 1_000.0, 500.0);
    program.prepare(sr);
    bench("ProgramDependent detector", &input, |x| program.tick(x));
    let mut sidechain = Sidechain::new(Some(100.0), Some(8_000.0));
    sidechain.prepare(sr);
    bench("Sidechain (HP + LP)", &input, |x| sidechain.tick(x));
    let curve = GainComputer {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 6.0,
    };
    for (name, topology) in [
        ("Compressor (feed-forward)", Topology::FeedForward),
        ("Compressor (feedback)", Topology::Feedback),
    ] {
        let mut comp = Compressor::new(curve, topology);
        comp.prepare(sr);
        bench(name, &input, |x| comp.tick(x * 4.0));
    }
    for (name, peaks) in [
        ("Limiter (true peak)", Peaks::True),
        ("Limiter (sample peak)", Peaks::Sample),
    ] {
        let mut limiter = Limiter::new(peaks, -1.0);
        limiter.prepare(sr);
        bench(name, &input, |x| limiter.tick(x * 4.0));
    }

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

    // F4: loading work, off the audio thread: per output sample.
    let hall = ImpulseResponse::mono(&input[..96_000]).recorded_at(SampleRate::new(44_100.0));
    let out_len = hall.resampled(SampleRate::new(48_000.0)).len();
    bench_points("ImpulseResponse resampled 44.1→48 kHz, 96 k taps", out_len, || {
        black_box(hall.resampled(SampleRate::new(48_000.0)));
    });
    let cab = ImpulseResponse::mono(&input[..4_096]);
    bench_points("ImpulseResponse minimum_phase 4096 taps", 4_096, || {
        black_box(cab.minimum_phase());
    });

    // F5: the STFT with an identity closure, 75 % overlap.
    for size in [1_024usize, 4_096] {
        let mut stft = Stft::new(size, size / 4, Window::Hann, |_: &mut [Complex<f32>]| {})
            .expect("Hann is COLA at a quarter hop");
        stft.prepare(sr);
        bench(&format!("Stft {size}/{} Hann", size / 4), &input, |x| stft.tick(x));
    }

    // F6: a designed FIR (80 dB Kaiser lowpass, 127 taps) run through
    // the convolver at zero latency.
    let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.1 }, 80.0, 0.04);
    let mut conv = Convolver::uniform(&fir.impulse_response(), 64, Head::Direct);
    conv.prepare(sr);
    bench(&format!("Fir {} taps B=64 direct", fir.len()), &input, |x| conv.tick(x));

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

    // Learned: Neural Amp Modeler's standard WaveNet (16 + 8 channels,
    // 2 x 10 layers, kernel 3, dilations 1..512), a lighter one (12 + 6
    // channels, 7 + 13 layers), and one-layer 16-unit LSTM and GRU.
    let array = |input: u32, channels: u32, head: u32, dilations: &str, bias: bool| {
        format!(
            r#"{{"input_size":{input},"condition_size":1,"channels":{channels},"head_size":{head},
            "kernel_size":3,"dilations":{dilations},"activation":"Tanh","gated":false,"head_bias":{bias}}}"#
        )
    };
    let wavenet = |layers: Vec<String>| {
        format!(
            r#"{{"version":"0.5.4","architecture":"WaveNet","sample_rate":48000,
            "config":{{"layers":[{}],"head":null,"head_scale":0.02}},"weights":{{w}}}}"#,
            layers.join(",")
        )
    };
    let decade = "[1,2,4,8,16,32,64,128,256,512]";
    let mut standard = network(&wavenet(vec![
        array(1, 16, 8, decade, false),
        array(16, 8, 1, decade, true),
    ]));
    standard.prepare(sr);
    bench("WaveNet (NAM standard)", &input, |x| standard.tick(x));
    let mut lite = network(&wavenet(vec![
        array(1, 12, 6, "[1,2,4,8,16,32,64]", false),
        array(12, 6, 1, "[128,256,512,1,2,4,8,16,32,64,128,256,512]", true),
    ]));
    lite.prepare(sr);
    bench("WaveNet (12 + 6 channels)", &input, |x| lite.tick(x));

    let mut lstm = network(
        r#"{"version":"0.5.4","architecture":"LSTM","sample_rate":48000,
        "config":{"input_size":1,"hidden_size":16,"num_layers":1},"weights":{w}}"#,
    );
    lstm.prepare(sr);
    bench("LSTM (1 x 16)", &input, |x| lstm.tick(x));

    let gru_weights = weights(3 * 16 * 17 + 6 * 16 + 17);
    let mut gru = Gru::parse(&format!(
        r#"{{"input_size":1,"hidden_size":16,"num_layers":1,"sample_rate":48000,"weights":{gru_weights}}}"#
    ))
    .unwrap();
    gru.prepare(sr);
    bench("GRU (1 x 16)", &input, |x| gru.tick(x));
    sheet::finish();
}

/// The slowest `block`-sample call of `process` over the input, after a
/// warm-up pass: the spike a host's deadline sees.
fn worst_block(name: &str, input: &[f32], block: usize, mut process: impl FnMut(&mut [f32])) {
    if !sheet::timed() {
        return;
    }
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
    if sheet::timed() {
        println!("  circuits: hand-derived topology, then Circuit<Dk> on its netlist");
    }

    let mut wdf = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::N4148,
        solver: PairSolver::default(),
    });
    wdf.prepare(sr);
    bench("DiodeClipper x10 (WDF)", input, |x| wdf.tick(10.0 * x));
    let mut clipper = dk(CLIPPER, sr);
    bench("Circuit<Dk> diode clipper x10", input, |x| clipper.tick(10.0 * x));
    solve_health(&clipper);

    let mut opamp = OpAmpClipper::new(OpAmpClipperConfig::TS808);
    opamp.prepare(sr);
    bench("OpAmpClipper", input, |x| opamp.tick(x));
    let mut opamp_dk = dk(OPAMP_CLIPPER, sr);
    bench("Circuit<Dk> op-amp clipper", input, |x| opamp_dk.tick(x));
    solve_health(&opamp_dk);

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
    solve_health(&cathode_b);
    let mut cathode_t = circuit_from(cathode_netlist(), sr);
    bench("Circuit<Dk> cathode x5 (Koren)", input, |x| cathode_t.tick(5.0 * x));
    solve_health(&cathode_t);

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

    // The transistor circuits (DSP.md E1, E2): no hand topology, the
    // ngspice decks themselves with their sources driven by the noise.
    println!("  transistor circuits: Circuit<Dk> on the ngspice decks");
    let mut fuzz = deck(include_str!("../../stave-test/spice/fuzz_face.cir"), sr);
    bench("Circuit<Dk> fuzz face", input, |x| fuzz.tick(x));
    solve_health(&fuzz);
    let mut ce = deck(include_str!("../../stave-test/spice/ce_stage.cir"), sr);
    bench("Circuit<Dk> CE stage x0.2", input, |x| ce.tick(0.2 * x));
    solve_health(&ce);
    let mut jfet = deck(include_str!("../../stave-test/spice/jfet_booster.cir"), sr);
    bench("Circuit<Dk> JFET booster", input, |x| jfet.tick(x));
    solve_health(&jfet);

    // E3: an EL34 class-AB pair, tabulated by hand and solved by DK.
    let mut pair = stave_dsp::PushPull::new();
    pair.set_pentode(stave_dsp::circuit::topologies::push_pull::PentodePair::EL34);
    pair.prepare(sr);
    bench("PushPull (EL34) x100", input, |x| pair.tick(100.0 * x));
    let mut pair_dk = dk(EL34_PUSH_PULL, sr);
    bench("Circuit<Dk> EL34 push-pull x100", input, |x| pair_dk.tick(100.0 * x));
    solve_health(&pair_dk);
    let mut pair_koren = circuit_from(el34_pair_netlist(), sr);
    bench("Circuit<Dk> EL34 push-pull x100 (Koren)", input, |x| pair_koren.tick(100.0 * x));
    solve_health(&pair_koren);
}

/// An ngspice deck as `Circuit<Dk>`, its `VIN` driven by the input.
fn deck(text: &str, sr: f32) -> Circuit<Dk> {
    let mut c = Circuit::<Dk>::new(&Netlist::parse_spice(text).unwrap()).unwrap();
    c.drive("vin").unwrap();
    c.prepare(sr);
    c
}

/// [`EL34_PUSH_PULL`] with stave's `pentode::Koren` in place of the
/// behavioural plate and screen sources.
fn el34_pair_netlist() -> Netlist {
    use stave_dsp::circuit::devices::pentode::koren::Koren as Pentode;
    let mut n = Netlist::parse_spice(EL34_PUSH_PULL).unwrap();
    for b in ["bp1", "bs1", "bp2", "bs2"] {
        n.remove(b).unwrap();
    }
    for (name, p, s, g) in [("x1", "p1", "s1", "g1"), ("x2", "p2", "s2", "g2")] {
        let kind = ElementKind::Pentode {
            plate: n.node(p).unwrap(),
            screen: n.node(s).unwrap(),
            grid: n.node(g).unwrap(),
            cathode: stave_dsp::circuit::netlist::Node::GROUND,
            tube: Pentode::EL34,
        };
        n.add(name, kind).unwrap();
    }
    n
}

/// `stave-test/spice/push_pull_pentode.cir`, its ramp the driven input.
const EL34_PUSH_PULL: &str = "EL34 push-pull pair
.param mu=11 ex=1.35 kg1=650 kg2=4200 kp=60 kvb=24
.param gg=6.177e-4 xi=1.314 cg=9.901 ig0=8.025e-8
V1 x 0 0
B1 in1 0 V = {-40 + V(x)}
B2 in2 0 V = {-40 - V(x)}
RS1 in1 g1 40k
RS2 in2 g2 40k
BG1 g1 0 I = {gg*pow(ln(1+exp(cg*V(g1)))/cg, xi) + ig0}
BG2 g2 0 I = {gg*pow(ln(1+exp(cg*V(g2)))/cg, xi) + ig0}
VB b 0 400
RA1 b p1 850
RA2 b p2 850
VS sup 0 400
RG1 sup s1 1k
RG2 sup s2 1k
BP1 p1 0 I = {V(p1) > 0 ? 2*pow(V(s1)/kp*ln(1+exp(kp*(1/mu+V(g1)/V(s1)))), ex)/kg1*atan(V(p1)/kvb) : 0}
BP2 p2 0 I = {V(p2) > 0 ? 2*pow(V(s2)/kp*ln(1+exp(kp*(1/mu+V(g2)/V(s2)))), ex)/kg1*atan(V(p2)/kvb) : 0}
BS1 s1 0 I = {V(g1)+V(s1)/mu > 0 ? pow(V(g1)+V(s1)/mu, ex)/kg2 : 0}
BS2 s2 0 I = {V(g2)+V(s2)/mu > 0 ? pow(V(g2)+V(s2)/mu, ex)/kg2 : 0}
.control
wrdata out v(p2,p1)
.endc";

/// The solve behind a `Circuit<Dk>` line: iterations per sample, cap
/// hits and continuation fallbacks over its timed passes.
fn solve_health(c: &Circuit<Dk>) {
    let h = c.health();
    println!(
        "  {:<28} {:>7.2} iterations/solve, {} capped, {} continuations",
        "",
        h.newton.mean_iterations(),
        h.newton.capped,
        h.continuations
    );
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
