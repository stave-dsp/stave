//! A representative set of stave-dsp stages as one runnable harness
//! (DSP.md P2, P3): a filter, the SVF, the oversampler (2× to 16×, the
//! factors circuits ship at), the FFT and the convolver, the triode
//! stage and the diode clipper, one plugin-sized chain, the expensive
//! models — the tape machine and the two op-amp pedals solved whole by
//! `Circuit<Dk>` — and Neural Amp Modeler's standard WaveNet, each run
//! block by block over the same seeded noise.
//!
//! ```text
//! stages list                   the stage ids, one per line, with what each is
//! stages count ID SAMPLES       run exactly SAMPLES samples (scripts/icount.sh)
//! stages run ID SECONDS         run for SECONDS of wall clock (scripts/profile.sh)
//! cargo bench -p stave-dsp --bench stages    a rough ns/sample of each
//! ```
//!
//! `scripts/icount.sh` counts the instructions `count` executes under
//! cachegrind at two lengths and takes the difference, so construction,
//! `prepare` and process start-up cancel and what is left is exactly the
//! per-sample cost, deterministic to the instruction. CI holds every
//! stage to the committed baseline `benches/stages.icount`.

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::{topologies, Circuit, Netlist};
use stave_dsp::physical::tape::{Chowdhury, Machine};
use stave_dsp::learned::nam;
use stave_dsp::primitives::spectral::{Complex, Head};
use stave_dsp::{
    Biquad, Convolver, DiodeClipper, DiodeClipperConfig, FilterSpec, Fft, ImpulseResponse,
    Oversampled, Oversampled2x, PairSolver, PassiveToneControlPositions, PassiveToneStack,
    PassiveToneStackComponents, Serial, Shockley, Stage, Svf, Triode,
};

const RATE: f32 = 48_000.0;
/// The block most stages run in (a host's typical buffer).
const BLOCK: usize = 256;
/// Length of the noise the blocks are cut from: a multiple of every block.
const NOISE: usize = 1 << 16;

/// One block, processed in place.
type Process = Box<dyn FnMut(&mut [f32])>;

struct Entry {
    id: &'static str,
    what: &'static str,
    /// Samples per call; `count`'s lengths are multiples of it.
    block: usize,
    build: fn() -> Process,
}

/// A prepared stage, run through its block form.
fn stage(mut s: impl Stage + 'static) -> Process {
    s.prepare(RATE);
    Box::new(move |b| s.process(b))
}

/// Nothing: the oversampler's own cost.
#[derive(Clone)]
struct Wire;

impl Stage for Wire {
    fn tick(&mut self, x: f32) -> f32 {
        x
    }
}

/// LCG noise in ±0.3, the datasheet's stimulus.
fn noise(len: usize) -> Vec<f32> {
    let mut lcg: u32 = 7;
    (0..len)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3
        })
        .collect()
}

fn convolver(taps: usize, block: usize, head: Head) -> Process {
    let ir = ImpulseResponse::mono(&noise(taps));
    stage(Convolver::uniform(&ir, block, head))
}

fn diode_clipper(solver: PairSolver) -> DiodeClipper {
    DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::N4148,
        solver,
    })
}

/// A 5150-style passive tone stack at noon.
fn tone_stack() -> PassiveToneStack {
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
    stack
}

/// A classic circuit's topology as `Circuit<Dk>`, its `vin` driven by
/// the input (the datasheet's `Circuit<Dk>` lines).
fn driven(netlist: Netlist) -> Process {
    let mut c = Circuit::<Dk>::new(&netlist).expect("the topology builds");
    c.drive("vin").expect("the topology has vin");
    stage(c)
}

/// Neural Amp Modeler's standard WaveNet (16 + 8 channels, 2 × 10
/// layers, kernel 3, dilations 1..512), weights in ±0.5 from an LCG —
/// the datasheet's "WaveNet (NAM standard)".
fn wavenet() -> Process {
    let array = |input: u32, channels: u32, head: u32, bias: bool| {
        format!(
            r#"{{"input_size":{input},"condition_size":1,"channels":{channels},"head_size":{head},
            "kernel_size":3,"dilations":[1,2,4,8,16,32,64,128,256,512],"activation":"Tanh","gated":false,"head_bias":{bias}}}"#
        )
    };
    let template = format!(
        r#"{{"version":"0.5.4","architecture":"WaveNet","sample_rate":48000,
        "config":{{"layers":[{},{}],"head":null,"head_scale":0.02}},"weights":WEIGHTS}}"#,
        array(1, 16, 8, false),
        array(16, 8, 1, true)
    );
    let expected = match nam::parse(&template.replace("WEIGHTS", "[]")) {
        Err(stave_dsp::learned::LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    };
    let mut lcg: u32 = 11;
    let w: Vec<String> = (0..expected)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            format!("{:.3}", (lcg >> 8) as f32 / (1u32 << 24) as f32 - 0.5)
        })
        .collect();
    let file = template.replace("WEIGHTS", &format!("[{}]", w.join(",")));
    stage(nam::parse(&file).expect("the network loads").network)
}

/// The swept Svf's loop: a retune and a tick per sample, the shape of a
/// modulated filter. Out of line so its assembly can be read by name
/// (`objdump -d --disassemble=<mangled svf_swept>`): DSP.md U4's
/// zero-cost check compares it before and after the setters took units.
#[inline(never)]
fn svf_swept(svf: &mut Svf, phase: &mut f32, block: &mut [f32]) {
    for x in block {
        *phase = if *phase >= 1.0 { 0.0 } else { *phase + 1.0 / 4_096.0 };
        svf.set(200.0 + 4_000.0 * *phase, 0.707);
        *x = svf.tick(*x);
    }
}

const STAGES: &[Entry] = &[
    Entry {
        id: "biquad",
        what: "Biquad lowpass, 1 kHz",
        block: BLOCK,
        build: || stage(Biquad::new(FilterSpec::Lowpass { hz: 1_000.0, q: 0.707 })),
    },
    Entry {
        id: "svf",
        what: "Svf (ZDF), 1 kHz",
        block: BLOCK,
        build: || stage(Svf::new(1_000.0, 0.707)),
    },
    Entry {
        id: "svf-swept",
        what: "Svf (ZDF) retuned every sample, 200 Hz to 4.2 kHz (a setter in the hot loop)",
        block: BLOCK,
        build: || {
            let mut svf = Svf::new(1_000.0, 0.707);
            svf.prepare(RATE);
            let mut phase = 0.0f32;
            Box::new(move |b| svf_swept(&mut svf, &mut phase, b))
        },
    },
    Entry {
        id: "oversampled2x",
        what: "Oversampled2x::front around nothing (the half-band pair)",
        block: BLOCK,
        build: || stage(Oversampled2x::front(Wire)),
    },
    Entry {
        id: "oversampled4x",
        what: "Oversampled2x::front(nested(..)) around nothing",
        block: BLOCK,
        build: || stage(Oversampled2x::front(Oversampled2x::nested(Wire))),
    },
    Entry {
        id: "oversampled8x",
        what: "Oversampled::linear(.., 8) around nothing (three half-band pairs)",
        block: BLOCK,
        build: || stage(Oversampled::linear(Wire, 8)),
    },
    Entry {
        id: "oversampled16x",
        what: "Oversampled::linear(.., 16) around nothing (four half-band pairs)",
        block: BLOCK,
        build: || stage(Oversampled::linear(Wire, 16)),
    },
    Entry {
        id: "fft-complex-1024",
        what: "Fft<f32> complex 1024, forward and inverse (per point)",
        block: 1024,
        build: || {
            let fft = Fft::<f32>::new(1024);
            let mut data = vec![Complex::default(); 1024];
            Box::new(move |b| {
                for (z, &x) in data.iter_mut().zip(b.iter()) {
                    *z = Complex::new(x, 0.0);
                }
                fft.forward(&mut data);
                fft.inverse(&mut data);
                for (x, z) in b.iter_mut().zip(&data) {
                    *x = z.re * (1.0 / 1024.0);
                }
            })
        },
    },
    Entry {
        id: "fft-real-4096",
        what: "Fft<f32> real 4096, forward and inverse (per point)",
        block: 4096,
        build: || {
            let fft = Fft::<f32>::new(4096);
            let mut bins = vec![Complex::default(); fft.real_bins()];
            Box::new(move |b| {
                fft.forward_real(b, &mut bins);
                fft.inverse_real(&mut bins, b);
            })
        },
    },
    Entry {
        id: "convolver-1k",
        what: "Convolver::uniform, 1024 taps, B=256 (a cabinet)",
        block: BLOCK,
        build: || convolver(1_024, 256, Head::Partitioned),
    },
    Entry {
        id: "convolver-16k",
        what: "Convolver::uniform, 16384 taps, B=256 (a room)",
        block: BLOCK,
        build: || convolver(16_384, 256, Head::Partitioned),
    },
    Entry {
        id: "convolver-1k-direct",
        what: "Convolver::uniform, 1024 taps, B=64, direct head (zero latency)",
        block: BLOCK,
        build: || convolver(1_024, 64, Head::Direct),
    },
    Entry {
        id: "convolver-96k",
        what: "Convolver::uniform, 96000 taps, B=256 (two seconds of hall)",
        block: BLOCK,
        build: || convolver(96_000, 256, Head::Partitioned),
    },
    Entry {
        id: "triode",
        what: "Triode stage (Koren 12AX7, first-order ADAA)",
        block: BLOCK,
        build: || stage(Triode::new()),
    },
    Entry {
        id: "diode-clipper-newton",
        what: "DiodeClipper (WDF), Newton",
        block: BLOCK,
        build: || stage(diode_clipper(PairSolver::default())),
    },
    Entry {
        id: "diode-clipper-omega",
        what: "DiodeClipper (WDF), Wright omega",
        block: BLOCK,
        build: || stage(diode_clipper(PairSolver::WrightOmega)),
    },
    Entry {
        id: "amp-chain",
        what: "A plugin-sized amp: highpass, two triodes at 2x, tone stack, 1024-tap cabinet",
        block: BLOCK,
        build: || {
            let ir = ImpulseResponse::mono(&noise(1_024));
            stage(Serial((
                Biquad::new(FilterSpec::Highpass { hz: 80.0, q: 0.707 }),
                Oversampled2x::front(Serial((Triode::new(), Triode::new()))),
                tone_stack(),
                Convolver::uniform(&ir, 256, Head::Partitioned),
            )))
        },
    },
    Entry {
        id: "tape",
        what: "Chowdhury tape (TC-260, 7.5 ips): record side 15x, flutter, play head",
        block: BLOCK,
        build: || stage(Chowdhury::new(Machine::TC260)),
    },
    Entry {
        id: "boss-ds1",
        what: "Circuit<Dk> Boss DS-1 (NJM2904), the whole netlist",
        block: BLOCK,
        build: || driven(topologies::boss_ds1()),
    },
    Entry {
        id: "mxr-phase90",
        what: "Circuit<Dk> MXR Phase 90 (LM741) at 3.4 V gates, the whole netlist",
        block: BLOCK,
        build: || driven(topologies::mxr_phase90(3.4)),
    },
    Entry {
        id: "wavenet",
        what: "WaveNet, Neural Amp Modeler's standard size (16 + 8 channels, 2 x 10 layers)",
        block: BLOCK,
        build: wavenet,
    },
];

/// Run `samples` samples (a multiple of the entry's block) of noise
/// through it; the result only keeps the work observable.
fn drive(entry: &Entry, process: &mut Process, input: &[f32], samples: usize) -> f32 {
    let mut buf = vec![0.0f32; entry.block];
    let (mut acc, mut at) = (0.0f32, 0);
    for _ in 0..samples / entry.block {
        buf.copy_from_slice(&input[at..at + entry.block]);
        process(black_box(&mut buf));
        acc += buf[0] + buf[entry.block - 1];
        at = (at + entry.block) % input.len();
    }
    black_box(acc)
}

/// Run for `seconds`; the measured ns/sample.
fn run_for(entry: &Entry, seconds: f64, input: &[f32]) -> f64 {
    let mut process = (entry.build)();
    let chunk = NOISE.max(entry.block);
    drive(entry, &mut process, input, chunk); // warm
    let (t0, mut samples) = (Instant::now(), 0usize);
    while t0.elapsed() < Duration::from_secs_f64(seconds) {
        drive(entry, &mut process, input, chunk);
        samples += chunk;
    }
    t0.elapsed().as_secs_f64() / samples as f64 * 1e9
}

const USAGE: &str = "usage: stages list | count ID SAMPLES | run ID SECONDS | --bench";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let find = |id: &str| STAGES.iter().find(|e| e.id == id);
    let input = noise(NOISE);
    match args.as_slice() {
        ["list"] => {
            for e in STAGES {
                println!("{}\t{}\t{}", e.id, e.block, e.what);
            }
        }
        ["count", id, samples] => {
            let (Some(entry), Ok(samples)) = (find(id), samples.parse::<usize>()) else {
                eprintln!("stages: no stage {id} (see `stages list`), or {samples} is not a count");
                return ExitCode::from(2);
            };
            if samples % entry.block != 0 {
                eprintln!("stages: {samples} is not a multiple of {id}'s block ({})", entry.block);
                return ExitCode::from(2);
            }
            let mut process = (entry.build)();
            drive(entry, &mut process, &input, samples);
        }
        ["run", id, seconds] => {
            let (Some(entry), Ok(seconds)) = (find(id), seconds.parse::<f64>()) else {
                eprintln!("stages: no stage {id} (see `stages list`), or {seconds} is not seconds");
                return ExitCode::from(2);
            };
            println!("{id}: {:.2} ns/sample", run_for(entry, seconds, &input));
        }
        // `cargo bench` passes --bench (and maybe a filter after it).
        ["--bench", filter @ ..] => {
            for e in STAGES.iter().filter(|e| filter.iter().all(|f| f.starts_with('-') || e.id.contains(f))) {
                println!("  {:<22} {:>8.2} ns/sample  {}", e.id, run_for(e, 0.5, &input), e.what);
            }
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
