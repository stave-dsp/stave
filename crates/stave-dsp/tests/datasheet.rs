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
//! per sample (the reference machine, 2026-11-12): diode clipper 264 vs 553,
//! op-amp clipper 296 vs 646, common cathode 1040 vs 1959 (the deck's
//! B-source Koren law; 2062 with the Koren device), tone stack 9.7 vs
//! 29. The general method costs 2–3× the hand derivation; a topology
//! stays its own type where that matters. The transistor decks have no
//! hand topology (the reference machine, 2026-11-12): Fuzz Face 3500
//! (two Gummel–Poon AC128s, four controls, 7.4 iterations on ±0.3 V
//! noise), CE stage 784, JFET booster 573.
//!
//! `Circuit<Wdf>` follows `Circuit<Dk>` on the same netlists (the
//! reference machine, 2026-11-13, DK in brackets): diode clipper 463
//! (510), op-amp clipper 549 (615), common cathode 1321 (1316), tone
//! stack 30 (28), Fuzz Face 3545 (3540), CE stage 823 (796), Tube
//! Screamer 2427 (2431). Both solve the same nonlinear equation with the
//! same devices, which is most of the cost: the wave digital form saves
//! where its nonlinear root is one port (a scalar solve, 9–11 %) and
//! spends a few ns on its tree passes elsewhere. The scattering iterative
//! method on the clipper costs 1847 (10.6 sweeps of two scalar solves).
//!
//! Stateful ADAA (DSP.md A1) against oversampling, at the alias levels
//! `aliasing_budgets.rs` records: closed form is cheaper at equal or
//! better aliasing (op-amp clipper 691 at 1× with ADAA vs 1117 at plain
//! 2×, 2236 at 4× with ADAA vs 3223 at plain 8×); quadrature is not
//! (Tube Screamer 3817 at 1× with ADAA vs 3953 at plain 2×, which aliases
//! 8 dB less).
//!
//! Knob moves (N6, `datasheet/knobs.rs`; the reference machine,
//! 2026-11-12): a low-rank update of the prepared matrices costs the tone
//! stack 133 ns a move (DK and WDF alike: 164 moving every sample against
//! 31 untouched) where a rebuild costs 60 µs (DK) / 70 µs (WDF), ~450×;
//! the Tube Screamer's drive 116 ns (DK) / 219 ns (WDF) a move against a
//! 845 µs (DK) / 860 µs (WDF) rebuild, 4000–7000×.
//!
//! Learned anchors (M3, 2026-11-12): WaveNet at Neural Amp Modeler's
//! standard size 2.2 µs/sample (12 + 6 channels 1.7), LSTM 1 x 16 0.32,
//! GRU 1 x 16 0.23 — a different scale: a capture network is the whole
//! amp, and costs like one. M7 (2026-11-13, measured beside them on a
//! loaded reference machine: LSTM 403 ns, GRU 284 in the same run): the
//! same networks at 44.1 kHz through `RatePolicy::Interpolated` 427 and
//! 299 ns, about 6% for the delay line; a slimmable 20-layer WaveNet 3.6 µs
//! at 16 channels, 1.5 at 4.
//!
//! Block models (M3, the reference machine, 2026-11-13): a
//! Wiener-Hammerstein model of two sections, a 24-knot spline and two
//! sections 41 ns/sample (32 with ADAA, whose uniform table is found
//! without the spline's knot search), the polynomial Hammerstein model
//! at order 5 on two sections a branch 18, on 256-tap FIR branches 709 —
//! against the circuits they were fitted to, 264-2431 above.
//!
//! Hardware beyond circuits (H1, H2; the reference machine, 2026-11-12):
//! the BBD line costs by its clock — HoltersParker with the Juno-60's
//! fifth-order filters 74 ns/sample at 40 kHz, 154 at 100 kHz; the
//! compandored line on fourth-order filters 60. A spring is its allpass
//! sections: 167 with the low chirps alone (100 sections), 566 with the
//! wide-band chirps (300); a three-spring tank 1.7 µs, which is where
//! Parker 2011's multirate structures would pay.
//!
//! H3, H4 (the reference machine, 2026-11-13, load 2.3): a plate costs
//! its modes, about 0.47 ns a mode for both pickups a block at a time —
//! an EMT 140 to 2.5 kHz (3175 modes) 1.5 µs/sample, to 5 kHz (6416) 3.1,
//! to 10 kHz (12 918) 6.1, to 20 kHz (25 955) 11.9; one `tick` at a time
//! streams the state through the cache each sample and costs 2.2× the
//! block form. A rotor with its back-wall image at two microphones 487
//! ns, a 122H cabinet (crossover, horn, drum) 980.
//!
//! E9, H7 (the reference machine, 2026-11-13, load 2.4): a loudspeaker
//! driver's fixed linear step 12.0 ns/sample; with Klippel's curves,
//! Newton on the 4-state trapezoid (two to three iterations) 268. Two
//! microphones of 8192-tap captures at zero latency 95; a close
//! microphone on the linear driver with its breakup 54; the live room's
//! six reflections and its late network 278.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::methods::wdf::{NonlinearRoot, Wdf};
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;
use stave_dsp::learned::block::{self, Nonlinearity, ParallelHammerstein, Section, WienerHammerstein};
use stave_dsp::learned::{nam, Gru, LoadError, RatePolicy};
#[path = "datasheet/sheet.rs"]
mod sheet;
#[path = "datasheet/knobs.rs"]
mod knobs;
#[path = "datasheet/bands.rs"]
mod bands;

use sheet::{bench, bench_points, N, RATE};
use stave_dsp::FilterSpec;
use stave_dsp::RcClipperConfig;
use stave_dsp::{AdaaOrder, AdsrConfig};
use stave_dsp::effects::dynamics::{
    Compressor, GainComputer, Limiter, Peak, Peaks, ProgramDependent, Rms, Sidechain, Topology,
};
use stave_dsp::effects::modulation::{FrequencyShifter, Sideband, SingleSideband};
use stave_dsp::circuit::topologies::diode_ladder::DiodeLadder;
use stave_dsp::circuit::topologies::transistor_ladder::TransistorLadder;
use stave_dsp::primitives::filters::hilbert::Hilbert;
use stave_dsp::primitives::filters::korg35::Korg35;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::primitives::filters::sallen_key::SallenKey;
use stave_dsp::units::{hz, seconds};
use stave_dsp::circuit::devices::bbd::{Chip, Filter, HoltersParker};
use stave_dsp::circuit::devices::compander::{self, Compander, Mode};
use stave_dsp::circuit::topologies::bucket_brigade::BucketBrigade;
use stave_dsp::effects::reverb::{Dattorro, Fdn, Kronecker, Mixing, Room, Schroeder, Shoebox, VelvetNoise, T60};
use stave_dsp::physical::cabinet::{Breakup, Captured, CloseMic, Placement, Position};
use stave_dsp::physical::plate::Plate;
use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
use stave_dsp::physical::rotary::{Cabinet, Directivity, Radiator, Rotor, Speed, Wall};
use stave_dsp::physical::spring::{Chirp, Spring, Tank};
use stave_dsp::physical::tape::{Chowdhury, Flutter, Formulation, Machine, Phase, Playback, Transport};
use stave_dsp::circuit::devices::photocell::{Najnudel, Vactrol};
use stave_dsp::circuit::topologies::opto_compressor::OptoCompressor;
use stave_dsp::units;
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
    Fft, Hysteresis, ImpulseResponse, Lag, Lfo, NonlinearRc, Osc, Oversampled, PairSolver,
    RandomWalk, Shockley, Slew, Stage, Waveform, Wavetable,
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
    adaa_datasheet(sr, &input);
    knobs::datasheet(sr, &input);

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
    // S3b: the per-band T60 (graphic-EQ attenuation, a hall profile),
    // the parametric matrix with its angles swept, and the velvet-noise
    // late reverb broadband and per band.
    let hall = T60::octaves([3.0, 2.8, 2.68, 2.55, 2.47, 2.5, 2.3, 1.89, 1.4, 1.2].map(seconds));
    let mut graphic = Fdn::new(Mixing::Hadamard);
    graphic.set_t60(hall);
    graphic.prepare(sr);
    bench("Fdn (8 lines, graphic EQ)", &input, |x| graphic.tick(x));
    let mut kronecker = Fdn::new(Mixing::Hadamard);
    kronecker.set_kronecker(Kronecker::hadamard());
    kronecker.set_angle_modulation(0.3, hz(0.5));
    kronecker.set_t60_s(2.0);
    kronecker.prepare(sr);
    bench("Fdn (8 lines, Kronecker, modulated)", &input, |x| kronecker.tick(x));
    let mut velvet = VelvetNoise::new();
    velvet.set_t60_s(2.0);
    velvet.prepare(sr);
    bench("VelvetNoise (4 branches, 408 taps)", &input, |x| velvet.tick(x));
    // Its block path: each tap reads a contiguous run, which vectorises.
    let mut blocks = input.to_vec();
    bench_points("VelvetNoise (4 branches, 408 taps, process)", blocks.len(), || {
        blocks.copy_from_slice(&input);
        for block in blocks.chunks_mut(64) {
            velvet.process(block);
        }
    });
    let mut velvet = VelvetNoise::new();
    velvet.set_t60(hall);
    velvet.prepare(sr);
    bench("VelvetNoise (graphic EQ)", &input, |x| velvet.tick(x));
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

    // S1: the nonlinear filters at 1 kHz, resonance 0.5, drive 0 dB
    // (Newton's iterations per sample printed); S4: the Hilbert pair at
    // its default design, the shifter and single sideband on it.
    let mut huovilainen = Huovilainen::new(hz(1_000.0), 0.5);
    huovilainen.prepare(sr);
    bench("Ladder (Huovilainen)", &input, |x| huovilainen.tick(x));
    println!("    {:.2} iterations/sample", huovilainen.stats().mean_iterations());
    let mut stinchcombe = Stinchcombe::new(hz(1_000.0), 0.5);
    stinchcombe.prepare(sr);
    bench("Ladder (Stinchcombe)", &input, |x| stinchcombe.tick(x));
    println!("    {:.2} iterations/sample", stinchcombe.stats().mean_iterations());
    let mut sallen_key = SallenKey::new(hz(1_000.0), 0.5);
    sallen_key.prepare(sr);
    bench("SallenKey", &input, |x| sallen_key.tick(x));
    println!("    {:.2} iterations/sample", sallen_key.stats().mean_iterations());
    let mut korg35 = Korg35::new(hz(1_000.0), 0.5);
    korg35.prepare(sr);
    bench("Korg35", &input, |x| korg35.tick(x));
    println!("    {:.2} iterations/sample", korg35.stats().mean_iterations());
    let mut hilbert = Hilbert::default();
    hilbert.prepare(sr);
    bench("Hilbert", &input, |x| {
        let (re, im) = hilbert.analytic(x);
        re + im
    });
    let mut shifter = FrequencyShifter::new(hz(100.0));
    shifter.prepare(sr);
    bench("FrequencyShifter", &input, |x| shifter.tick(x));
    let mut ssb = SingleSideband::new(Sideband::Upper);
    ssb.prepare(sr);
    let mut carrier = Osc::new(Waveform::Sine);
    carrier.set_hz(440.0);
    carrier.prepare(sr);
    bench("SingleSideband", &input, |x| ssb.modulate(x, carrier.tick(0.0)));

    // H1: bucket-brigade delay (the cost rises with the clock).
    for clock in [40_000.0, 100_000.0] {
        let mut line = HoltersParker::new(Chip::MN3007, Filter::JUNO_60_INPUT, Filter::JUNO_60_OUTPUT);
        line.prepare(sr);
        line.set_clock(units::hz(clock));
        let name = format!("HoltersParker (MN3007, Juno-60 filters, {} kHz clock)", clock / 1_000.0);
        bench(&name, &input, |x| line.tick(x));
    }
    let mut bbd = BucketBrigade::new(Chip::MN3007);
    bbd.prepare(sr);
    bbd.set_clock(units::hz(40_000.0));
    bench("BucketBrigade (MN3007, NE570, 40 kHz clock)", &input, |x| bbd.tick(x));
    let mut compressor = Compander::new(compander::Chip::NE570, Mode::Compress);
    compressor.prepare(sr);
    bench("Compander (NE570)", &input, |x| compressor.tick(x));

    // H2: spring reverb.
    let mut chirp = Chirp::new(100, 6, 0.6);
    chirp.prepare(sr);
    bench("Chirp (100 sections, k = 6)", &input, |x| chirp.tick(x));
    let mut spring = Spring::new(units::seconds(0.033), units::hz(4_000.0));
    spring.prepare(sr);
    bench("Spring (low and wide-band chirps)", &input, |x| spring.tick(x));
    spring.set_high_chirps(units::db(-200.0));
    bench("Spring (low chirps only)", &input, |x| spring.tick(x));
    let mut tank = Tank::default();
    tank.prepare(sr);
    bench("Tank (3 springs)", &input, |x| tank.tick(x));

    // H3: plate reverb, both pickups, block by block; the cost is the
    // mode count, linear in the bandwidth (EMT 140: 1.3 modes a hertz).
    let block: Vec<f32> = input[..512].to_vec();
    let (mut left, mut right) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    for (top, name) in [
        (2_500.0, "Plate (EMT 140, to 2.5 kHz)"),
        (5_000.0, "Plate (EMT 140, to 5 kHz)"),
        (10_000.0, "Plate (EMT 140, to 10 kHz)"),
        (20_000.0, "Plate (EMT 140, to 20 kHz)"),
    ] {
        let mut plate = Plate::emt140(units::seconds(3.0));
        plate.set_bandwidth(units::hz(top));
        plate.prepare(sr);
        if sheet::timed() {
            println!("  {name}: {} modes", plate.modes().len());
        }
        bench_points(name, block.len(), || plate.process_stereo(&block, &mut left, &mut right));
    }
    let mut plate = Plate::emt140(units::seconds(3.0));
    plate.set_bandwidth(units::hz(5_000.0));
    plate.prepare(sr);
    bench("Plate (EMT 140, to 5 kHz, mono tick)", &input, |x| plate.tick(x));

    // H4: rotary speaker at tremolo, both microphones.
    let mut horn: Radiator = Radiator::new(Rotor::HORN_122H, Directivity::HORN_122H);
    horn.set_walls(&[Wall::BACK_122H]);
    horn.set_speed(Speed::Tremolo);
    horn.prepare(sr);
    bench("Radiator (horn, back wall, stereo)", &input, |x| {
        let (l, r) = horn.tick_stereo(x);
        l + r
    });
    let mut leslie: Cabinet = Cabinet::leslie_122h();
    leslie.set_speed(Speed::Tremolo);
    leslie.prepare(sr);
    bench("Cabinet (Leslie 122H, stereo)", &input, |x| {
        let (l, r) = leslie.tick_stereo(x);
        l + r
    });

    // E9: a loudspeaker driver (Jensen C12N, LR-2 coil) at 20 V a unit:
    // the fixed linear step, and Newton on Klippel's curves.
    let mut speaker = Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR);
    speaker.set_volts(20.0);
    speaker.prepare(sr);
    bench("Driver (Thiele–Small)", &input, |x| speaker.tick(x));
    let mut speaker = Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::from_limits(1.0e-3, 1.2e-3, 6e-3));
    speaker.set_volts(20.0);
    speaker.prepare(sr);
    bench("Driver (large signal)", &input, |x| speaker.tick(x));

    // H7: two microphones of 8192-tap captures (synthetic decaying
    // noise), zero latency; a close microphone on the driver; the room.
    let capture = |seed: u32| {
        let mut ir = vec![0.0f32; 8_192];
        noise(&mut ir);
        let mut lcg = seed;
        for (n, v) in ir.iter_mut().enumerate() {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            *v *= (-(n as f32) / 600.0).exp() * if lcg & 1 == 0 { 1.0 } else { -1.0 };
        }
        ImpulseResponse::mono(&ir)
    };
    let mut cab = Captured::new(vec![
        vec![Position::new(0.0, capture(1)), Position::new(1.0, capture(2))],
        vec![Position::new(0.0, capture(3))],
    ]);
    cab.set_placement(0, 0.5);
    cab.prepare(sr);
    bench("Captured (2 mics, 8k taps)", &input, |x| cab.tick(x));
    let mut mic = CloseMic::new(
        Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR),
        Placement::CLOSE,
        Breakup::PAPER_12,
    );
    mic.prepare(sr);
    bench("CloseMic (C12N, cardioid, breakup)", &input, |x| mic.tick(x));
    let mut room = Room::new(Shoebox::LIVE_ROOM);
    room.prepare(sr);
    bench("Room (live room, early + late)", &input, |x| room.tick(x));

    // E7: photocells. The cell's TR-BDF2 step under a held LED current,
    // and under a 5 Hz tremolo's sweep (the Newton iterations move).
    let mut vactrol = Vactrol::new(Najnudel::VTL5C3);
    vactrol.prepare(sr);
    bench("Vactrol (VTL5C3), 10 mA", &input, |_| vactrol.step(0.010) as f32);
    let mut lfo = 0.0f64;
    let lfo_step = 5.0 / f64::from(sr);
    bench("Vactrol (VTL5C3), tremolo", &input, |_| {
        lfo = (lfo + lfo_step).fract();
        vactrol.step(0.010 * (1.0 + (std::f64::consts::TAU * lfo).sin())) as f32
    });
    let mut opto = OptoCompressor::new(Najnudel::VTL5C3);
    opto.prepare(sr);
    bench("OptoCompressor (VTL5C3)", &input, |x| opto.tick(10.0 * x));

    // E8: tape. The machine (record side 15× at 48 kHz, flutter, play
    // head), and its linear parts alone.
    let mut deck = Chowdhury::new(Machine::TC260);
    deck.prepare(sr);
    bench("Chowdhury tape (TC-260, 7.5 ips)", &input, |x| deck.tick(x));
    let mut play = Playback::new(Machine::TC260.play, Formulation::FERRIC.thickness, Machine::ips(7.5), Phase::Linear);
    play.prepare(sr);
    bench("Playback losses (7.5 ips)", &input, |x| play.tick(x));
    let mut flutter = Flutter::new(Transport::AKAI_4000D);
    flutter.prepare(sr);
    bench("Flutter (Akai 4000D)", &input, |x| flutter.tick(x));

    // H8: vintage digital. A word-length reduction with TPDF dither,
    // and shaped; a sampler's converters at its own clock; an early
    // digital delay's fixed-point datapath.
    {
        use stave_dsp::circuit::devices::converter::{self, Converter};
        use stave_dsp::primitives::fixed::Overflow;
        use stave_dsp::primitives::quantise::{Dither, Quantiser, Shaping};
        let mut tpdf = Quantiser::new(16).with_dither(Dither::Triangular, 1);
        bench("Quantiser 16-bit TPDF", &input, |x| tpdf.tick(x));
        let mut shaped = Quantiser::new(16)
            .with_dither(Dither::Triangular, 1)
            .with_shaping(Shaping::SECOND_ORDER)
            .unwrap();
        bench("Quantiser 16-bit TPDF 2nd-order shaped", &input, |x| shaped.tick(x));
        let mut sp = Converter::new(converter::Machine::SP1200);
        sp.prepare(sr);
        bench("Converter SP-1200", &input, |x| sp.tick(x));
        let mut delay = converter::Delay::new(converter::Machine::SP1200, 0.5, Overflow::Saturate, 8);
        delay.set_seconds(0.3);
        delay.set_feedback(0.6);
        delay.prepare(sr);
        bench("Digital delay 12-bit", &input, |x| delay.tick(x));
    }

    // H9: slow random processes.
    {
        use stave_dsp::primitives::control::drift::{OrnsteinUhlenbeck, Wiener};
        let mut ou = OrnsteinUhlenbeck::new(25.0, 0.5, 30.0, 1);
        ou.prepare(sr);
        bench("OrnsteinUhlenbeck", &input, |x| ou.tick(x));
        let mut wiener = Wiener::new(1.0, 1e-4, 1e-3, 1);
        wiener.prepare(sr);
        bench("Wiener", &input, |x| wiener.tick(x));
    }

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
    bands::datasheet(sr, &input);

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

    // A3: the triangle's corners, the tabulated residuals, a hard-synced
    // saw (a reset every master cycle) and the mip-mapped table.
    let mut osc_tri = Osc::new(Waveform::Triangle);
    osc_tri.prepare(sr);
    osc_tri.set_hz(220.0);
    bench("Osc (triangle, polyBLAMP)", &input, |x| osc_tri.tick(x));
    let mut osc_min = Osc::min_blep(Waveform::Saw);
    osc_min.prepare(sr);
    osc_min.set_hz(220.0);
    bench("Osc (saw, minBLEP)", &input, |x| osc_min.tick(x));
    let mut osc_sync = Osc::new(Waveform::Saw);
    osc_sync.prepare(sr);
    osc_sync.set_hz(370.0);
    osc_sync.set_sync(Some(stave_dsp::units::Hz::new(220.0)));
    bench("Osc (saw, hard sync)", &input, |x| osc_sync.tick(x));
    let mut table = Wavetable::new(Waveform::Saw);
    table.prepare(sr);
    table.set_hz(220.0);
    bench("Wavetable (saw)", &input, |x| table.tick(x));

    // A2: each oversampling region around nothing (the filters alone),
    // in blocks, per outer sample.
    for (phase, factor) in [
        ("linear", 2),
        ("linear", 3),
        ("linear", 4),
        ("linear", 6),
        ("linear", 8),
        ("minimum", 2),
        ("minimum", 4),
        ("minimum", 8),
    ] {
        let mut os = if phase == "linear" {
            Oversampled::linear((), factor)
        } else {
            Oversampled::minimum((), factor)
        };
        os.prepare(sr);
        let mut block = input[..4_096].to_vec();
        bench_points(&format!("Oversampled {phase} {factor}x (wire)"), block.len(), || {
            os.process(black_box(&mut block))
        });
    }

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

    // R19's perceptual metrics, per analysed sample at 48 kHz (offline:
    // the cost of a fingerprint or a budget, not of a plugin): half a
    // second of programme against a copy with a little distortion.
    {
        use stave_measure::perceptual::{erb::ErbError, gedlee::GedLee, nmr::Nmr, rnonlin::rnonlin, Playback};
        let len = 24_000;
        let a: Vec<f64> = input[..len].iter().map(|&x| f64::from(x)).collect();
        let b: Vec<f64> = a.iter().map(|x| x + 0.05 * x * x * x).collect();
        bench_points("Nmr (BS.1387 basic, 48 kHz)", len, || {
            black_box(Nmr::new(Playback::default()).between(&a, &b, 48_000.0));
        });
        bench_points("ErbError (ERB bands, 48 kHz)", len, || {
            black_box(ErbError::between(&a, &b, 48_000.0));
        });
        bench_points("Rnonlin (40 gammatone bands, 48 kHz)", len, || {
            black_box(rnonlin(&a, &b, 48_000.0));
        });
        let harmonics: Vec<Complex<f64>> = (1..=15).map(|h| Complex::new(1.0 / f64::from(h * h), 0.1 / f64::from(h))).collect();
        bench_points("GedLee (15 harmonics)", 1, || {
            black_box(GedLee::of_harmonics(&harmonics));
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

    // S5: the EQs on six bands (cuts, shelves, bells); the dynamic EQ
    // with its bands working on the noise.
    {
        use stave_dsp::effects::eq::{
            Band, Detector, Dynamic, DynamicBand, Dynamics, LinearPhase, MinimumPhase, Phase,
        };
        use stave_dsp::units::{db, ms, Ratio};
        let bands = [
            Band::LowCut { frequency: hz(30.0), q: 0.707 },
            Band::LowShelf { frequency: hz(120.0), gain: db(4.0) },
            Band::Bell { frequency: hz(400.0), gain: db(-6.0), q: 1.5 },
            Band::Bell { frequency: hz(3_000.0), gain: db(5.0), q: 3.0 },
            Band::HighShelf { frequency: hz(10_000.0), gain: db(-3.0) },
        ];
        let mut minimum = MinimumPhase::new(&bands);
        minimum.prepare(sr);
        bench("MinimumPhase EQ (5 bands)", &input, |x| minimum.tick(x));
        let mut linear = LinearPhase::new(&bands);
        linear.prepare(sr);
        bench("LinearPhase EQ 85 ms", &input, |x| linear.tick(x));
        let working = Dynamics {
            threshold: db(-30.0),
            ratio: Ratio::new(4.0),
            knee: db(6.0),
            range: db(-9.0),
            attack: ms(2.0),
            release: ms(60.0),
            detector: Detector::Peak,
        };
        let dynamic = |phase: Phase| {
            let mut eq = Dynamic::new(&[
                DynamicBand::dynamic(bands[1], Phase::Minimum, working),
                DynamicBand::dynamic(bands[2], Phase::Minimum, working),
                DynamicBand::dynamic(bands[3], phase, working),
                DynamicBand::dynamic(bands[4], Phase::Minimum, working),
            ]);
            eq.prepare(sr);
            eq
        };
        let mut eq = dynamic(Phase::Minimum);
        bench("Dynamic EQ (4 minimum-phase bands)", &input, |x| eq.tick(x));
        let mut eq = dynamic(Phase::Linear);
        bench("Dynamic EQ (3 min + 1 linear band)", &input, |x| eq.tick(x));
    }

    // S6: the pitch shifters a fifth up, and a granulator.
    {
        use stave_dsp::effects::granular::Granulator;
        use stave_dsp::effects::pitch::{PhaseVocoder, Psola, Reconstruction};
        use stave_dsp::units::{ms, seconds, semitones};
        for (r, name) in [
            (Reconstruction::Prusa, "PhaseVocoder +7 st (Prusa)"),
            (Reconstruction::LarocheDolson, "PhaseVocoder +7 st (LarocheDolson)"),
        ] {
            let mut pv = PhaseVocoder::new(r, semitones(12.0));
            pv.set_shift(semitones(7.0));
            pv.prepare(sr);
            bench(name, &input, |x| pv.tick(x));
        }
        // A voiced input, so the grains run pitch-synchronously.
        let voiced: Vec<f32> = (0..input.len())
            .map(|n| {
                let p = core::f32::consts::TAU * 150.0 * n as f32 / sr;
                (1..20).map(|k| (k as f32 * p).sin() / k as f32).sum::<f32>() * 0.3
            })
            .collect();
        let mut psola = Psola::new(hz(60.0), hz(1_000.0));
        psola.set_shift(semitones(7.0));
        psola.prepare(sr);
        bench("Psola +7 st", &voiced, |x| psola.tick(x));
        let mut cloud = Granulator::new(seconds(1.0), 16);
        cloud.set_density(50.0);
        cloud.set_size(ms(80.0));
        cloud.set_pitch(semitones(7.0));
        cloud.set_position(ms(150.0));
        cloud.set_spray(ms(50.0));
        cloud.prepare(sr);
        bench("Granulator 50 grains/s, 80 ms", &input, |x| cloud.tick(x));
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

    // M7: the same recurrent networks, trained at 48 kHz, at 44.1 kHz
    // through RatePolicy::Interpolated (the state read from a cubic
    // Lagrange delay line), and a slimmable WaveNet at its smallest and
    // full widths (one 20-layer array, 16 channels, sliced).
    let mut lstm_44 = lstm.clone();
    lstm_44.set_rate_policy(RatePolicy::Interpolated);
    lstm_44.prepare(44_100.0);
    bench("LSTM (1 x 16) Interpolated 48 -> 44.1 kHz", &input, |x| {
        lstm_44.tick(x)
    });
    let mut gru_44 = gru.clone();
    gru_44.set_rate_policy(RatePolicy::Interpolated);
    gru_44.prepare(44_100.0);
    bench("GRU (1 x 16) Interpolated 48 -> 44.1 kHz", &input, |x| {
        gru_44.tick(x)
    });
    let slim_text = |w: &str| {
        format!(
            r#"{{"version":"0.7.0","architecture":"WaveNet","sample_rate":48000,
            "config":{{"layers":[{{"input_size":1,"condition_size":1,"channels":16,
            "head":{{"out_channels":1,"kernel_size":1,"bias":true}},"kernel_size":3,
            "dilations":[1,2,4,8,16,32,64,128,256,512,1,2,4,8,16,32,64,128,256,512],
            "activation":"Tanh","gated":false,
            "slimmable":{{"method":"slice_channels_uniform","kwargs":{{"allowed_channels":[4,8,12,16]}}}}}}],
            "head":null,"head_scale":0.02}},"weights":{w}}}"#
        )
    };
    let needed = match nam::parse(&slim_text("[]")) {
        Err(LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    };
    let slim = nam::parse(&slim_text(&weights(needed))).unwrap();
    for (i, name) in [
        (0, "WaveNet slimmable 1 x 20 layers at 4 of 16 channels"),
        (3, "WaveNet slimmable 1 x 20 layers at 16 of 16 channels"),
    ] {
        let mut net = slim.network_at(i);
        net.prepare(sr);
        bench(name, &input, |x| net.tick(x));
    }

    // M3: block models at the sizes stave's circuits fit to (the Tube
    // Screamer's W-H: two sections before a 24-knot spline, one after;
    // the polynomial Hammerstein at order 5), against the circuits above.
    let section = |hz: f64| {
        let w = std::f64::consts::TAU * hz / f64::from(sr);
        let alpha = w.sin() / 1.4;
        let a0 = 1.0 + alpha;
        Section {
            b: [(1.0 - w.cos()) / 2.0 / a0, (1.0 - w.cos()) / a0, (1.0 - w.cos()) / 2.0 / a0],
            a: [-2.0 * w.cos() / a0, (1.0 - alpha) / a0],
        }
    };
    let knots: Vec<f64> = (0..24).map(|k| (f64::from(k) / 23.0 * 2.0 - 1.0).powi(3) * 3.0).collect();
    let spline = Nonlinearity::Spline {
        values: knots.iter().map(|&u| u.tanh()).collect(),
        knots,
    };
    let mut wh = WienerHammerstein::new(
        sr,
        block::Linear::Rational(vec![section(700.0), section(5_000.0)]),
        spline,
        block::Linear::Rational(vec![section(9_000.0), section(12_000.0)]),
    );
    wh.prepare(sr);
    bench("WienerHammerstein (2 + 2 sections, 24-knot spline)", &input, |x| wh.tick(x));
    wh.set_antialiasing(true);
    bench("WienerHammerstein (2 + 2 sections, 24-knot spline, ADAA)", &input, |x| wh.tick(x));
    let mut parallel = ParallelHammerstein::new(
        sr,
        (0..5).map(|m| block::Linear::Rational(vec![section(2_000.0 + 1_000.0 * f64::from(m)); 2])).collect(),
    );
    parallel.prepare(sr);
    bench("ParallelHammerstein (order 5, 2 sections a branch)", &input, |x| parallel.tick(x));
    let mut fir = ParallelHammerstein::new(sr, (0..5).map(|m| block::Linear::Fir(vec![0.01 / f64::from(m + 1); 256])).collect());
    fir.prepare(sr);
    bench("ParallelHammerstein (order 5, 256 taps a branch)", &input, |x| fir.tick(x));
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

/// The same netlist as `Circuit<Wdf>`, `source` driven by the input.
fn wdf_from(netlist: &Netlist, source: &str, sr: f32) -> Circuit<Wdf> {
    let mut c = Circuit::<Wdf>::new(netlist).unwrap();
    c.drive(source).unwrap();
    c.prepare(sr);
    c
}

fn wdf_deck(deck: &str, sr: f32) -> Circuit<Wdf> {
    wdf_from(&Netlist::parse_spice(deck).unwrap(), "v1", sr)
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
        println!("  circuits: hand-derived topology, then Circuit<Dk> and Circuit<Wdf> on its netlist");
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
    let mut clipper_wdf = wdf_deck(CLIPPER, sr);
    bench("Circuit<Wdf> diode clipper x10", input, |x| clipper_wdf.tick(10.0 * x));
    solve_health(&clipper_wdf);
    let mut clipper_sim = wdf_deck(CLIPPER, sr);
    clipper_sim.set_nonlinear_root(NonlinearRoot::Scattering).unwrap();
    clipper_sim.prepare(sr);
    bench("Circuit<Wdf> diode clipper x10 (SIM)", input, |x| clipper_sim.tick(10.0 * x));
    solve_health(&clipper_sim);

    let mut opamp = OpAmpClipper::new(OpAmpClipperConfig::TS808);
    opamp.prepare(sr);
    bench("OpAmpClipper", input, |x| opamp.tick(x));
    let mut opamp_dk = dk(OPAMP_CLIPPER, sr);
    bench("Circuit<Dk> op-amp clipper", input, |x| opamp_dk.tick(x));
    solve_health(&opamp_dk);
    let mut opamp_wdf = wdf_deck(OPAMP_CLIPPER, sr);
    bench("Circuit<Wdf> op-amp clipper", input, |x| opamp_wdf.tick(x));
    solve_health(&opamp_wdf);

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
    let mut cathode_wdf = wdf_from(&cathode_netlist(), "v1", sr);
    bench("Circuit<Wdf> cathode x5 (Koren)", input, |x| cathode_wdf.tick(5.0 * x));
    solve_health(&cathode_wdf);

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
    // H9: the same with its resistors' thermal noise, a column each.
    let mut noisy = dk(TONE_STACK, sr);
    noisy.set_noise(Some(stave_dsp::circuit::methods::dk::Noise::room(1)));
    bench("Circuit<Dk> tone stack, thermal noise", input, |x| noisy.tick(x));
    let mut stack_wdf = wdf_deck(TONE_STACK, sr);
    bench("Circuit<Wdf> tone stack", input, |x| stack_wdf.tick(x));

    // The transistor circuits (DSP.md E1, E2): no hand topology, the
    // ngspice decks themselves with their sources driven by the noise.
    println!("  transistor circuits: Circuit<Dk> on the ngspice decks");
    // Each static (the cards' DC part), then with the cards' charge
    // storage (DSP.md R11): one more device output per stored charge.
    let mut fuzz = deck(include_str!("../../stave-test/spice/fuzz_face_static.cir"), sr);
    bench("Circuit<Dk> fuzz face", input, |x| fuzz.tick(x));
    solve_health(&fuzz);
    let mut fuzz_c = deck(include_str!("../../stave-test/spice/fuzz_face.cir"), sr);
    bench("Circuit<Dk> fuzz face (capacitances)", input, |x| fuzz_c.tick(x));
    solve_health(&fuzz_c);
    let fuzz_deck = Netlist::parse_spice(include_str!("../../stave-test/spice/fuzz_face_static.cir")).unwrap();
    let mut fuzz_wdf = wdf_from(&fuzz_deck, "vin", sr);
    bench("Circuit<Wdf> fuzz face", input, |x| fuzz_wdf.tick(x));
    solve_health(&fuzz_wdf);
    let mut ce = deck(include_str!("../../stave-test/spice/ce_stage_static.cir"), sr);
    bench("Circuit<Dk> CE stage x0.2", input, |x| ce.tick(0.2 * x));
    solve_health(&ce);
    let mut ce_c = deck(include_str!("../../stave-test/spice/ce_stage.cir"), sr);
    bench("Circuit<Dk> CE stage x0.2 (capacitances)", input, |x| ce_c.tick(0.2 * x));
    solve_health(&ce_c);
    let ce_deck = Netlist::parse_spice(include_str!("../../stave-test/spice/ce_stage_static.cir")).unwrap();
    let mut ce_wdf = wdf_from(&ce_deck, "vin", sr);
    bench("Circuit<Wdf> CE stage x0.2", input, |x| ce_wdf.tick(0.2 * x));
    solve_health(&ce_wdf);
    let mut jfet = deck(include_str!("../../stave-test/spice/jfet_booster_static.cir"), sr);
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

    // E4: the Tube Screamer's clipping stage on the JRC4558 macromodel.
    let mut screamer = circuit_from(tube_screamer_netlist(), sr);
    bench("Circuit<Dk> Tube Screamer (JRC4558)", input, |x| screamer.tick(4.5 + x));
    solve_health(&screamer);
    let mut screamer_wdf = wdf_from(&tube_screamer_netlist(), "v1", sr);
    bench("Circuit<Wdf> Tube Screamer (JRC4558)", input, |x| screamer_wdf.tick(4.5 + x));
    solve_health(&screamer_wdf);

    // E5: an LM13700 VCA at a fixed control voltage.
    let mut vca = circuit_from(ota_vca_netlist(), sr);
    bench("Circuit<Dk> OTA VCA (LM13700) x15", input, |x| vca.tick(15.0 * x));
    solve_health(&vca);

    // S1: the ladder circuits at resonance 2 and 10, ±30 mV in.
    let mut transistor_ladder =
        driven(TransistorLadder { resonance: 2.0, ..TransistorLadder::DEFAULT }.netlist(), sr);
    bench("Circuit<Dk> transistor ladder", input, |x| transistor_ladder.tick(0.1 * x));
    solve_health(&transistor_ladder);
    let mut diode_ladder =
        driven(DiodeLadder { resonance: 10.0, ..DiodeLadder::DEFAULT }.netlist(), sr);
    bench("Circuit<Dk> diode ladder", input, |x| diode_ladder.tick(0.1 * x));
    solve_health(&diode_ladder);

    // R24: the classic circuits, their topologies' netlists, input driven.
    println!("  classic circuits: Circuit<Dk> on circuit::topologies");
    use stave_dsp::circuit::topologies;
    let mut muff = driven(topologies::big_muff_rams_head(), sr);
    bench("Circuit<Dk> Big Muff (Ram's Head)", input, |x| muff.tick(x));
    solve_health(&muff);
    let mut ts808 = driven(topologies::tube_screamer_ts808(), sr);
    bench("Circuit<Dk> Tube Screamer TS-808", input, |x| ts808.tick(x));
    solve_health(&ts808);
    let mut rat = driven(topologies::proco_rat(), sr);
    bench("Circuit<Dk> ProCo RAT (LM308)", input, |x| rat.tick(x));
    solve_health(&rat);
    let mut jcm = driven(topologies::marshall_jcm800_2203(), sr);
    bench("Circuit<Dk> JCM800 2203 preamp", input, |x| jcm.tick(x));
    solve_health(&jcm);
    let mut moog = driven(topologies::moog_ladder_904a(3.0), sr);
    bench("Circuit<Dk> Moog ladder (k = 3)", input, |x| moog.tick(x));
    solve_health(&moog);
    let mut ce2 = driven(topologies::boss_ce2_filters(), sr);
    bench("Circuit<Dk> CE-2 BBD filters", input, |x| ce2.tick(x));
    solve_health(&ce2);
    let mut phase90 = driven(topologies::mxr_phase90(3.4), sr);
    bench("Circuit<Dk> MXR Phase 90 (LM741)", input, |x| phase90.tick(x));
    solve_health(&phase90);
    let mut ds1 = driven(topologies::boss_ds1(), sr);
    bench("Circuit<Dk> Boss DS-1 (NJM2904)", input, |x| ds1.tick(x));
    solve_health(&ds1);
    let mut klon = driven(topologies::klon_centaur(0.5, 0.5), sr);
    bench("Circuit<Dk> Klon Centaur (TL072)", input, |x| klon.tick(x));
    solve_health(&klon);
}

/// A topology's netlist as `Circuit<Dk>`, its `vin` driven by the input.
fn driven(netlist: Netlist, sr: f32) -> Circuit<Dk> {
    let mut c = Circuit::<Dk>::new(&netlist).unwrap();
    c.drive("vin").unwrap();
    c.prepare(sr);
    c
}

/// Stateful ADAA (DSP.md A1) against oversampling, for the alias levels
/// `aliasing_budgets.rs` records: each circuit at 1×, 2× and 4× with ADAA
/// beside 2×, 4× and 8× without (the plain 1× lines are above). The
/// clippers' means are closed form (one solve a sample), the Tube
/// Screamer's by quadrature (two).
fn adaa_datasheet(sr: f32, input: &[f32]) {
    use stave_dsp::circuit::methods::adaa::Antialiasing;
    if sheet::timed() {
        println!("  stateful ADAA vs oversampling");
    }
    for factor in [1u32, 2, 4, 8] {
        for adaa in [false, true] {
            if (factor == 1 && !adaa) || (factor == 8 && adaa) {
                continue;
            }
            let antialiasing = if adaa { Antialiasing::Adaa } else { Antialiasing::None };
            let suffix = format!(" {factor}x{}", if adaa { " ADAA" } else { "" });

            let mut wdf = DiodeClipper::new(DiodeClipperConfig {
                input_ohms: 2.2e3,
                capacitor_farads: 10e-9,
                diode: Shockley::N4148,
                solver: PairSolver::default(),
            });
            wdf.set_antialiasing(antialiasing);
            let mut wdf = Oversampled::linear(wdf, factor);
            wdf.prepare(sr);
            bench(&format!("DiodeClipper x10 (WDF){suffix}"), input, |x| wdf.tick(10.0 * x));

            let circuits: [(&str, Netlist, f32, f32); 3] = [
                ("Circuit<Dk> diode clipper x10", Netlist::parse_spice(CLIPPER).unwrap(), 10.0, 0.0),
                ("Circuit<Dk> op-amp clipper", Netlist::parse_spice(OPAMP_CLIPPER).unwrap(), 1.0, 0.0),
                ("Circuit<Dk> Tube Screamer (JRC4558)", tube_screamer_netlist(), 1.0, 4.5),
            ];
            for (name, netlist, gain, offset) in circuits {
                let mut c = Circuit::<Dk>::new(&netlist).unwrap();
                c.drive("v1").unwrap();
                c.set_antialiasing(antialiasing);
                let mut os = Oversampled::linear(c, factor);
                os.prepare(sr);
                bench(&format!("{name}{suffix}"), input, |x| os.tick(offset + gain * x));
            }
        }
    }
}

/// An ngspice deck as `Circuit<Dk>`, its `VIN` driven by the input.
fn deck(text: &str, sr: f32) -> Circuit<Dk> {
    let mut c = Circuit::<Dk>::new(&Netlist::parse_spice(text).unwrap()).unwrap();
    c.drive("vin").unwrap();
    c.prepare(sr);
    c
}

/// `stave-test/spice/tube_screamer.cir` with stave's op-amp element in
/// place of the behavioural one, its input driven.
fn tube_screamer_netlist() -> Netlist {
    use stave_dsp::circuit::devices::opamp::{Amp, Macromodel};
    let mut n = Netlist::parse_spice(
        "TS808 clipping stage, JRC4558
V1 in 0 4.5
RG n 0 1meg
R1 fb s 4.7k
C1 s 0 47n
RF out fb 551k
CF out fb 51p
D1 out fb D1N914
D2 fb out D1N914
.model D1N914 D(Is=2.52n Rs=.568 N=1.752)
.control
wrdata out v(out)
.endc",
    )
    .unwrap();
    let kind = ElementKind::OpAmp {
        plus: n.node("in").unwrap(),
        minus: n.node("fb").unwrap(),
        output: n.node("out").unwrap(),
        amp: Amp::Macromodel(Macromodel::JRC4558),
        supply: (0.0, 9.0),
    };
    n.add("u1", kind).unwrap();
    n
}

/// `stave-test/spice/ota_vca.cir` with stave's OTA element, the control
/// held at 5 V (about 0.24 mA of bias), the signal driven.
fn ota_vca_netlist() -> Netlist {
    use stave_dsp::circuit::devices::ota::BipolarPair;
    let mut n = Netlist::parse_spice(
        "LM13700 VCA
V1 sig 0 0
RIN sig inp 33k
RP inp 0 330
RM inm 0 330
VC c 0 5
RC c abc 15k
RL out 0 10k
.control
wrdata out v(out)
.endc",
    )
    .unwrap();
    let kind = ElementKind::Ota {
        plus: n.node("inp").unwrap(),
        minus: n.node("inm").unwrap(),
        output: n.node("out").unwrap(),
        bias: n.node("abc").unwrap(),
        negative: stave_dsp::circuit::netlist::Node::GROUND,
        ota: BipolarPair::LM13700,
    };
    n.add("u1", kind).unwrap();
    n
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
            capacitances: stave_dsp::circuit::devices::pentode::Interelectrode::NONE,
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
fn solve_health(c: &impl CircuitHealth) {
    let h = c.circuit_health();
    println!(
        "  {:<28} {:>7.2} iterations/solve, {} capped, {} continuations",
        "",
        h.newton.mean_iterations(),
        h.newton.capped,
        h.continuations
    );
}

/// The solve account both methods keep.
trait CircuitHealth {
    fn circuit_health(&self) -> stave_dsp::circuit::methods::dk::Health;
}

impl CircuitHealth for Circuit<Dk> {
    fn circuit_health(&self) -> stave_dsp::circuit::methods::dk::Health {
        self.health()
    }
}

impl CircuitHealth for Circuit<Wdf> {
    fn circuit_health(&self) -> stave_dsp::circuit::methods::dk::Health {
        self.health()
    }
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
        ("t1", ElementKind::Triode {
            plate: p,
            grid: g,
            cathode: k,
            tube: Tube::Koren(Koren::T12AX7),
            capacitances: stave_dsp::circuit::devices::triode::Interelectrode::NONE,
        }),
    ];
    for (name, kind) in parts {
        n.add(name, kind).unwrap();
    }
    n.probe = Some(Probe::node(p));
    n
}
