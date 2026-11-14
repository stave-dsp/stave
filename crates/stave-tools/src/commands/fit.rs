//! The capture workflow for block-oriented models (DSP.md M3, M4):
//! `fit.stimulus` writes the stimulus to play through a device; `fit`
//! fits a Wiener–Hammerstein or polynomial Hammerstein model to a
//! recording of it, to a hosted plugin or stage, or to a stave circuit,
//! writes the model file `stave_dsp::learned::block::parse` reads, and
//! reports how well it fits and where it falls short. Recordings are
//! read at run time and never committed. The work is
//! `stave_measure::identify`.

use std::path::{Path, PathBuf};

use serde_json::json;
use stave_dsp::learned::block::{Block, Linear};
use stave_dsp::Stage;
use stave_host::wav;
use stave_measure::identify::capture::{Capture, Measurements, Program, Segment};
use stave_measure::identify::hammerstein;
use stave_measure::identify::wiener_hammerstein::{self as wh, Structure};
use stave_measure::identify::Validation;

use crate::command::{causes, Artefact, CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};

const PROGRAM: Field = Field::flag(
    "program",
    Kind::String,
    "Also play ROLE (a reference-corpus role, DSP.md R22, e.g. guitar_di; ROLE#N for item N), held out for validation",
);
const PROGRAM_DB: Field = Field::flag("program_db", Kind::Number, "The program material's RMS, dB re --reference").default("0");
const REFERENCE: Field = Field::flag("reference", Kind::Number, "The input RMS of 0 dB").default("0.05");

pub const STIMULUS: CommandInfo = CommandInfo {
    id: "fit.stimulus",
    title: "Write a capture stimulus",
    summary: "Write the capture stimulus to play through a device, and its description beside it",
    description: "Write the capture stimulus (mono, 32-bit float) to OUT and OUT.capture, its description. \
                  0 dB is an input RMS of --reference (default 0.05, -26 dBFS): multisines at -20, -6, 0, \
                  +6, +12 dB, a synchronized sweep peaking at the 0 dB sine's peak, noise bursts held out \
                  for validation, and with --program real program material, held out too. Play it \
                  through the device and record the output from the start; then `stave fit --stimulus \
                  OUT --capture RECORDING`.",
    inputs: &[
        Field::positional("out", Kind::String, "The stimulus WAV to write"),
        Field::flag("rate", Kind::Number, "Sample rate, Hz").default("48000"),
        REFERENCE,
        PROGRAM,
        PROGRAM_DB,
    ],
    outputs: &[
        Field::flag("out", Kind::String, "The stimulus WAV").required(),
        Field::flag("description", Kind::String, "Its .capture description file").required(),
        Field::flag("seconds", Kind::Number, "Its length").required(),
        Field::flag("rate", Kind::Number, "Hz").required(),
        Field::flag("peak", Kind::Number, "Its peak, full scale 1").required(),
    ],
    long_running: false,
};

pub const FIT: CommandInfo = CommandInfo {
    id: "fit",
    title: "Fit a block-oriented model",
    summary: "Fit a Wiener-Hammerstein or polynomial Hammerstein model to a recording, a plugin or a circuit",
    description: "Fit to one source and write the model file:\n\
                  - a recording of the stimulus through a device (--stimulus and --capture): channel 1 the \
                  device's output, channel 2 (optional) the interface's loopback of the stimulus. The \
                  latency is --latency, else the cross-correlation peak of the loopback (exact), else of \
                  the device's output (which adds the device's group delay);\n\
                  - a plugin or catalog stage (--plugin PATH or stage:NAME), hosted offline at 48 kHz, its \
                  reported latency compensated;\n\
                  - a stave circuit (--circuit NAME): the R17 deck crates/stave-test/spice/NAME.cir through \
                  Circuit<Dk> at --oversample x 48 kHz, the levels relative to the deck's knee.\n\
                  Reports the fit's error at each level, the BLA's shape against level, and the held-out \
                  bursts' (and program material's) simulation error, worst ERB and NMR.",
    inputs: &[
        Field::flag("out", Kind::String, "The model file to write (JSON)").required(),
        Field::flag("stimulus", Kind::String, "The stimulus WAV (`fit.stimulus` wrote it and its .capture)"),
        Field::flag("capture", Kind::String, "The recording of the stimulus through the device"),
        Field::flag("plugin", Kind::String, "A plugin or stage:NAME to fit, hosted offline"),
        Field::flag("circuit", Kind::String, "A circuit deck name (crates/stave-test/spice/NAME.cir)"),
        Field::flag(
            "structure",
            Kind::Choice(&["wiener-hammerstein", "wiener", "hammerstein", "parallel"]),
            "The model: parallel is Novak's polynomial Hammerstein, from the sweep",
        )
        .default("wiener-hammerstein"),
        Field::flag("order", Kind::Integer, "parallel: the highest power").default("5"),
        Field::flag("max_order", Kind::Integer, "The rational fits' highest order").default("5"),
        Field::flag("knots", Kind::Integer, "The curve's spline knots").default("24"),
        REFERENCE,
        Field::flag("oversample", Kind::Integer, "circuit: the oversampling factor (a power of two)").default("4"),
        Field::flag("latency", Kind::Integer, "The recording's delay in samples (default: measured)"),
        Field::flag("export_pair", Kind::String, "Also write DIR/input.wav and DIR/output.wav, aligned, for training a network out of tree"),
        PROGRAM,
        PROGRAM_DB,
    ],
    outputs: &[
        Field::flag("out", Kind::String, "The model file").required(),
        Field::flag("source", Kind::String, "What was fitted").required(),
        Field::flag("structure", Kind::String, "The model structure").required(),
        Field::flag("report", Kind::Strings, "The fit's report, line by line").required(),
        Field::flag("bursts", Kind::Json, "Held-out bursts: level_db, error_db, erb_max_db, nmr_total_db").required(),
        Field::flag("program", Kind::Json, "Held-out program material, the same measures"),
    ],
    long_running: true,
};

fn fail(m: impl Into<String>) -> CommandError {
    CommandError::Failed(m.into())
}

fn usage(m: impl Into<String>) -> CommandError {
    CommandError::Usage(m.into())
}

fn io(path: &Path, e: impl std::fmt::Display) -> CommandError {
    fail(format!("{}: {e}", path.display()))
}

/// `OUT.wav.capture`.
fn sidecar(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".capture");
    PathBuf::from(s)
}

/// The corpus material `--program` names, at the capture's rate, mono,
/// with 10 ms fades, at an RMS of `--program-db` re the reference.
fn program(input: &Input, capture: &Capture, cx: &mut Context<'_>) -> Result<Option<Program>, CommandError> {
    let Some(spec) = input.opt_str("program") else { return Ok(None) };
    let (role, index) = match spec.split_once('#') {
        Some((r, n)) => (r, n.parse::<usize>().map_err(|_| usage(format!("program {spec}: ROLE or ROLE#N")))?),
        None => (spec, 0),
    };
    let corpus = stave_test::corpus::Corpus::open().map_err(|s| fail(format!("program {spec}: {s}")))?;
    if corpus.spec.role(role).is_none() {
        let names: Vec<&str> = corpus.spec.roles.iter().map(|r| r.name.as_str()).collect();
        return Err(usage(format!("program: no role {role} (roles: {})", names.join(", "))));
    }
    let items = corpus.role(role).map_err(|s| fail(format!("program {spec}: {s}")))?;
    let item = items.get(index).ok_or_else(|| usage(format!("program: role {role} has {} items", items.len())))?;
    let m = corpus.load(item).map_err(fail)?;
    let channels = m.at_rate(capture.rate);
    let n = channels[0].len();
    let k = channels.len() as f64;
    let mut x: Vec<f64> = (0..n).map(|i| channels.iter().map(|c| f64::from(c[i])).sum::<f64>() / k).collect();
    let fade = ((0.01 * capture.rate) as usize).min(n / 2);
    for i in 0..fade {
        let g = 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / fade as f64).cos();
        x[i] *= g;
        x[n - 1 - i] *= g;
    }
    let rms = (x.iter().map(|v| v * v).sum::<f64>() / n.max(1) as f64).sqrt();
    if rms <= 0.0 {
        return Err(fail(format!("program {spec}: silent")));
    }
    let gain = capture.reference * 10f64.powf(input.number("program_db").unwrap_or(0.0) / 20.0) / rms;
    x.iter_mut().for_each(|v| *v *= gain);
    cx.progress(0.0, None, format!("program material: {}", m.describe()));
    Ok(Some(Program {
        name: format!("{role}#{index} {}", m.name),
        samples: x,
    }))
}

pub fn stimulus(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let out = input.path("out").ok_or_else(|| usage("fit.stimulus: out is required"))?;
    let mut capture = Capture::standard(input.number("rate").unwrap_or(48_000.0), input.number("reference").unwrap_or(0.05));
    capture.program = program(input, &capture, cx)?;
    let x = capture.samples();
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if peak >= 1.0 {
        return Err(usage(format!("the stimulus peaks at {peak:.2} (over full scale): lower --reference")));
    }
    wav::write(&out, &wav::Audio {
        sample_rate: capture.rate as u32,
        channels: vec![x.iter().map(|&v| v as f32).collect()],
    })
    .map_err(|e| io(&out, causes(&e)))?;
    let side = sidecar(&out);
    std::fs::write(&side, capture.describe()).map_err(|e| io(&side, e))?;
    let seconds = x.len() as f64 / capture.rate;
    let text = format!(
        "{}: {seconds:.1} s at {} Hz, peak {peak:.3}; {} beside it describes it",
        out.display(),
        capture.rate,
        side.display()
    );
    Ok(Outcome::ok(
        json!({ "out": out, "description": side, "seconds": seconds, "rate": capture.rate, "peak": peak }),
        text,
    )?
    .with(Artefact::new(&out, "audio/wav", "stimulus"))
    .with(Artefact::new(&side, "text/plain", "description")))
}

/// A recording's rate, its first channel (the device), and its second
/// when there is one (the interface's loopback).
type Recording = (u32, Vec<f64>, Option<Vec<f64>>);

fn recording(path: &Path) -> Result<Recording, CommandError> {
    let audio = wav::read(path).map_err(|e| io(path, causes(&e)))?;
    let channel = |i: usize| audio.channels.get(i).map(|c| c.iter().map(|&v| f64::from(v)).collect::<Vec<f64>>());
    Ok((audio.sample_rate, channel(0).ok_or_else(|| io(path, "no channels"))?, channel(1)))
}

/// The measurements and the capture design, from a plugin, a circuit or
/// a recording.
fn measure(input: &Input, cx: &mut Context<'_>) -> Result<(Capture, Measurements, String), CommandError> {
    let reference = input.number("reference").unwrap_or(0.05);
    if let Some(path) = input.opt_str("plugin") {
        let mut capture = Capture::standard(48_000.0, reference);
        capture.program = program(input, &capture, cx)?;
        let config = stave_host::config::HostConfig {
            sample_rate: stave_host::config::SampleRate::constant(48_000.0),
            ..stave_host::config::HostConfig::default()
        };
        let mut opened = stave_host::opened::Opened::open(Path::new(path), config).map_err(|e| fail(causes(&e)))?;
        let inst = opened.instance();
        let layout = inst.layout();
        if layout.inputs == 0 || layout.outputs == 0 {
            return Err(fail(format!("{path} has no audio input or output")));
        }
        let latency = inst.latency() as usize;
        let x = capture.samples();
        let mut recorded = Vec::with_capacity(x.len() + latency);
        let block = layout.max_block.max(1);
        let silence = vec![0.0f32; block];
        let mut outs = vec![vec![0.0f32; block]; layout.outputs];
        let feed: Vec<f32> = x.iter().map(|&v| v as f32).chain(std::iter::repeat_n(0.0, latency)).collect();
        cx.progress(0.0, None, format!("hosting {path}: {latency} samples of reported latency"));
        for chunk in feed.chunks(block) {
            let n = chunk.len();
            let inputs: Vec<&[f32]> = (0..layout.inputs).map(|c| if c == 0 { chunk } else { &silence[..n] }).collect();
            let mut outputs: Vec<&mut [f32]> = outs.iter_mut().map(|o| &mut o[..n]).collect();
            inst.process(&inputs, &mut outputs, &[]);
            recorded.extend(outs[0][..n].iter().map(|&v| f64::from(v)));
        }
        let m = capture.read(&recorded, latency).map_err(|e| fail(e.to_string()))?;
        let what = format!("{path} hosted offline at 48 kHz, 0 dB = {:.3} RMS in", capture.reference);
        return Ok((capture, m, what));
    }
    if let Some(name) = input.opt_str("circuit") {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../stave-test/spice");
        let text = std::fs::read_to_string(dir.join(format!("{name}.plan")))
            .map_err(|e| usage(format!("no plan for circuit {name} ({e}); the R17 decks are in crates/stave-test/spice")))?;
        let plan = stave_test::characterise::Plan::parse(&text).map_err(|e| fail(format!("{name}.plan: {e}")))?;
        let knee = plan.knee.ok_or_else(|| fail("the plan has no knee"))?.amplitude;
        let factor = input.integer("oversample").unwrap_or(4) as usize;
        if !factor.is_power_of_two() {
            return Err(usage("oversample must be a power of two"));
        }
        let mut capture = Capture::standard(48_000.0, knee / std::f64::consts::SQRT_2);
        capture.program = program(input, &capture, cx)?;
        let deck = stave_test::characterise::deck(name);
        let plan_input = plan.input.clone();
        let rate = 48_000.0 * factor as f64;
        cx.progress(0.0, None, format!("measuring {name} at {rate} Hz (0 dB = its knee, {knee:.3e} V peak)"));
        let m = capture.run(factor, move || {
            let mut c = stave_test::characterise::model::circuit(&deck, &plan_input, rate);
            Box::new(move |x| c.tick_f64(x))
        });
        return Ok((capture, m, format!("Circuit<Dk> on spice/{name}.cir at {factor}x, 0 dB = its knee")));
    }
    let stim = input.path("stimulus").ok_or_else(|| usage("fit: --stimulus and --capture, --plugin or --circuit"))?;
    let rec = input.path("capture").ok_or_else(|| usage("fit: --capture (the recording)"))?;
    let side = sidecar(&stim);
    let text = std::fs::read_to_string(&side)
        .map_err(|e| io(&side, format!("{e} (`stave fit stimulus` writes it beside the stimulus)")))?;
    let mut capture = Capture::parse(&text).map_err(|e| io(&side, e))?;
    if input.has("program") {
        return Err(usage("--program goes on `stave fit stimulus` for a recording (the stimulus file holds it)"));
    }
    // The program material's samples are the stimulus file's own.
    let layout = capture.layout();
    if let (Some(p), Some(Segment::Program { start, len })) = (&mut capture.program, layout.last()) {
        let (_, played, _) = recording(&stim)?;
        p.samples = played
            .get(*start..start + len)
            .ok_or_else(|| io(&stim, "shorter than its description"))?
            .to_vec();
    }
    let (rate, recorded, loopback) = recording(&rec)?;
    if f64::from(rate) != capture.rate {
        return Err(fail(format!("the recording is at {rate} Hz, the stimulus at {} Hz", capture.rate)));
    }
    let (latency, how) = match (input.integer("latency"), &loopback) {
        (Some(v), _) => (v as usize, "given"),
        (None, Some(l)) => (capture.align(l, capture.rate as usize), "from the loopback channel"),
        (None, None) => (
            capture.align(&recorded, capture.rate as usize),
            "from the device's output (its group delay included: record a loopback on channel 2 or pass --latency)",
        ),
    };
    cx.progress(0.0, None, format!("the recording is {latency} samples late ({how})"));
    let m = capture.read(&recorded, latency).map_err(|e| fail(e.to_string()))?;
    Ok((capture, m, format!("the recording {} ({latency} samples of latency)", rec.display())))
}

pub fn fit(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let out = input.path("out").ok_or_else(|| usage("fit: --out MODEL.json"))?;
    let structure = input.str("structure")?.to_string();
    let max_order = input.integer("max_order").unwrap_or(5) as usize;
    let (capture, m, what) = measure(input, cx)?;
    cx.checkpoint()?;
    cx.progress(0.0, None, format!("fitting {structure}"));
    let mut lines = Vec::new();
    let block: Block = if structure == "parallel" {
        let options = hammerstein::Options {
            order: input.integer("order").unwrap_or(5) as usize,
            branches: hammerstein::Branches::Rational {
                max_order,
                tolerance_db: 0.1,
            },
            ..hammerstein::Options::default()
        };
        let f = hammerstein::fit(&m.sweep, capture.rate, &m.sweep_input, &m.sweep_output, &options)
            .ok_or_else(|| fail("the sweep did not separate into harmonics (too short for this order?)"))?;
        lines.push(format!("polynomial Hammerstein, order {}, kernels {} samples", options.order, f.kernel_len));
        lines.push(format!("dropped below the floor: {:?}", f.dropped));
        lines.push(format!(
            "branch fits (worst, dB re each kernel's peak): {}",
            f.branch_error_db.iter().map(|e| format!("{e:.2}")).collect::<Vec<_>>().join(", ")
        ));
        lines.push(format!("input limit {:.4e} (the sweep's peak)", f.model.limit()));
        f.model.into()
    } else {
        let options = wh::Options {
            structure: match structure.as_str() {
                "wiener" => Structure::Wiener,
                "hammerstein" => Structure::Hammerstein,
                _ => Structure::WienerHammerstein,
            },
            max_order,
            knots: input.integer("knots").unwrap_or(24) as usize,
            ..wh::Options::default()
        };
        let f = wh::fit(&m.records, &options).ok_or_else(|| fail("no fit (the best linear approximation would not fit)"))?;
        lines.push(format!(
            "{structure}: BLA {} zeros / {} poles (worst {:.2} dB), split {} (best of {}), curve {}monotone",
            f.bla.zeros.len(),
            f.bla.poles.len(),
            f.bla_error_db,
            f.split,
            f.splits_tried,
            if f.monotone { "" } else { "not " }
        ));
        for (rms, e) in &f.error_db {
            lines.push(format!("  {:+5.1} dB: in-band error {e:.1} dB", 20.0 * (rms / capture.reference).log10()));
        }
        lines.push("BLA shape against level (a Wiener-Hammerstein system's moves only in gain):".into());
        for s in &f.shapes {
            lines.push(format!(
                "  {:+5.1} dB: {:.2} dB at {:.0} Hz, {:.1} deg",
                20.0 * (s.rms / capture.reference).log10(),
                s.db,
                s.at_hz,
                s.degrees
            ));
        }
        f.model.into()
    };
    cx.checkpoint()?;
    cx.progress(0.0, None, "validating on the held-out bursts");
    let mut model = block.clone();
    model.prepare(capture.rate as f32);
    let y: Vec<f64> = m.bursts_input.iter().map(|&x| model.tick_f64(x)).collect();
    lines.push("held-out bursts (simulation error / ERB worst / NMR total):".into());
    let mut bursts = Vec::new();
    for (db, span) in capture.burst_spans() {
        let v = Validation::of(&m.bursts_output[span.clone()], &y[span], capture.rate);
        lines.push(format!("  {db:+5.1} dB: {:.1} dB / {:.2} dB / {:.1} dB", v.error_db, v.erb.max_db, v.nmr.total_db));
        bursts.push(json!({ "level_db": db, "error_db": v.error_db, "erb_max_db": v.erb.max_db, "nmr_total_db": v.nmr.total_db }));
    }
    let mut held_program = None;
    if let Some(p) = &capture.program {
        let mut model = block.clone();
        model.prepare(capture.rate as f32);
        let y: Vec<f64> = m.program_input.iter().map(|&x| model.tick_f64(x)).collect();
        let v = Validation::of(&m.program_output, &y, capture.rate);
        lines.push(format!(
            "held-out program material {} (simulation error / ERB worst / NMR total): {:.1} dB / {:.2} dB / {:.1} dB",
            p.name, v.error_db, v.erb.max_db, v.nmr.total_db
        ));
        held_program = Some(json!({ "name": p.name, "error_db": v.error_db, "erb_max_db": v.erb.max_db, "nmr_total_db": v.nmr.total_db }));
    }
    let sections = |l: &Linear| match l {
        Linear::Rational(s) => s.len(),
        Linear::Fir(_) => 0,
    };
    if let Block::WienerHammerstein(w) = &block {
        lines.push(format!("sections: {} before the curve, {} after", sections(w.input()), sections(w.output())));
    }
    let note = format!("fitted by stave fit to {what}\n{}", lines.join("\n"));
    std::fs::write(&out, block.to_json(&note)).map_err(|e| io(&out, e))?;
    let mut text = format!("{note}\nwrote {}", out.display());
    let mut outcome_artefacts = vec![Artefact::new(&out, "application/json", "model")];
    if let Some(dir) = input.path("export_pair") {
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let (x, y) = export_pair(&dir, &capture, &m)?;
        text.push_str(&format!("\nwrote {} and {}", x.display(), y.display()));
        outcome_artefacts.push(Artefact::new(x, "audio/wav", "training input"));
        outcome_artefacts.push(Artefact::new(y, "audio/wav", "training output"));
    }
    let mut outcome = Outcome::ok(
        json!({ "out": out, "source": what, "structure": structure, "report": lines, "bursts": bursts, "program": held_program }),
        text,
    )?;
    outcome.artefacts = outcome_artefacts;
    Ok(outcome)
}

/// The measured multisines, sweep, bursts and program material back to
/// back, input and output aligned: a training pair for a network fitted
/// out of tree.
fn export_pair(dir: &Path, capture: &Capture, m: &Measurements) -> Result<(PathBuf, PathBuf), CommandError> {
    let (mut x, mut y) = (Vec::new(), Vec::new());
    for r in &m.records.records {
        x.extend(r.input.iter().map(|&v| v as f32));
        y.extend(r.output.iter().map(|&v| v as f32));
    }
    x.extend(m.sweep_input.iter().map(|&v| v as f32));
    y.extend(m.sweep_output.iter().map(|&v| v as f32));
    x.extend(m.bursts_input.iter().map(|&v| v as f32));
    y.extend(m.bursts_output.iter().map(|&v| v as f32));
    x.extend(m.program_input.iter().map(|&v| v as f32));
    y.extend(m.program_output.iter().map(|&v| v as f32));
    let rate = capture.rate as u32;
    let (xp, yp) = (dir.join("input.wav"), dir.join("output.wav"));
    wav::write(&xp, &wav::Audio { sample_rate: rate, channels: vec![x] }).map_err(|e| io(&xp, causes(&e)))?;
    wav::write(&yp, &wav::Audio { sample_rate: rate, channels: vec![y] }).map_err(|e| io(&yp, causes(&e)))?;
    Ok((xp, yp))
}
