//! stave-fit — the capture workflow for block-oriented models (DSP.md M3,
//! M4). Writes the stimulus to play through a device; fits a
//! Wiener–Hammerstein or polynomial Hammerstein model to a recording of
//! it, or to a stave circuit run directly; writes the model file
//! `stave_dsp::learned::block::parse` reads, and reports how well it fits
//! and where it falls short. Recordings are read at run time and never
//! committed. The work is `stave_measure::identify`; this binary parses
//! arguments, reads and writes files, and prints.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use stave_dsp::learned::block::{Block, Linear};
use stave_dsp::Stage;
use stave_host::wav;
use stave_measure::identify::capture::{Capture, Measurements};
use stave_measure::identify::hammerstein;
use stave_measure::identify::wiener_hammerstein::{self as wh, Structure};
use stave_measure::identify::Validation;

const USAGE: &str = "\
stave-fit — capture and fit block-oriented models (DSP.md M3, M4)

USAGE:
  stave-fit stimulus OUT.wav [--rate HZ] [--reference RMS]
      Write the capture stimulus (mono, 32-bit float) and OUT.wav.capture,
      its description. 0 dB is an input RMS of --reference (default 0.05,
      -26 dBFS): multisines at -20, -6, 0, +6, +12 dB, a synchronized sweep
      peaking at the 0 dB sine's peak, noise bursts held out for validation.
      Play it through the device and record the output from the start.

  stave-fit fit --stimulus OUT.wav --capture RECORDING.wav -o MODEL.json [options]
      Fit to a recording of the stimulus through a device: channel 1 the
      device's output, channel 2 (optional) the interface's loopback of the
      stimulus. The latency is --latency, else the cross-correlation peak
      of the loopback (exact), else of the device's output (which adds the
      device's group delay). Read at run time only.

  stave-fit fit --plugin PATH -o MODEL.json [options]
      Fit to a plugin (a .vst3 bundle) or a catalogued stave stage
      (stage:NAME, e.g. stage:clipper/diode), hosted offline at 48 kHz:
      the stimulus played through it, its reported latency compensated.

  stave-fit fit --circuit NAME -o MODEL.json [options]
      Fit to a stave circuit: the R17 deck crates/stave-test/spice/NAME.cir
      through Circuit<Dk> at --oversample x 48 kHz (default 4), the levels
      relative to the deck's knee (its plan).

OPTIONS (fit):
  --structure S      wiener-hammerstein (default), wiener, hammerstein, or
                     parallel (Novak's polynomial Hammerstein, from the sweep)
  --order N          parallel: the highest power (default 5)
  --max-order N      the rational fits' highest order (default 5)
  --knots N          the curve's spline knots (default 24)
  --reference RMS    --plugin: the input RMS of 0 dB (default 0.05)
  --latency N        the recording's delay in samples (default: measured)
  --export-pair DIR  also write DIR/input.wav and DIR/output.wav, aligned,
                     for training a network out of tree (Neural Amp Modeler)
  -h, --help         this text
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stave-fit: {e}");
            let mut source = e.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

/// `--name value` pairs and positional words.
struct Args {
    words: Vec<String>,
    named: Vec<(String, String)>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self, String> {
        let (mut words, mut named) = (Vec::new(), Vec::new());
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a.starts_with('-') {
                let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
                named.push((a.trim_start_matches('-').to_string(), v.clone()));
            } else {
                words.push(a.clone());
            }
        }
        Ok(Self { words, named })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.named.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        self.get(name)
            .map_or(Ok(default), |v| v.parse().map_err(|_| format!("--{name}: not a number: {v}")))
    }

    fn check(&self, known: &[&str]) -> Result<(), String> {
        match self.named.iter().find(|(k, _)| !known.contains(&k.as_str())) {
            Some((k, _)) => Err(format!("unknown option --{k} (see --help)")),
            None => Ok(()),
        }
    }
}

fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let a = Args::parse(args)?;
    match a.words.first().map(String::as_str) {
        Some("stimulus") => stimulus(&a),
        Some("fit") => fit(&a),
        Some(other) => Err(format!("unknown command {other} (see --help)").into()),
        None => Err("no command (see --help)".into()),
    }
}

fn sidecar(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".capture");
    PathBuf::from(s)
}

fn stimulus(a: &Args) -> Result<(), Box<dyn Error>> {
    a.check(&["rate", "reference"])?;
    let out = PathBuf::from(a.words.get(1).ok_or("stimulus: no output file")?);
    let capture = Capture::standard(a.number("rate", 48_000.0)?, a.number("reference", 0.05)?);
    let x = capture.samples();
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if peak >= 1.0 {
        return Err(format!("the stimulus peaks at {peak:.2} (over full scale): lower --reference").into());
    }
    wav::write(&out, &wav::Audio {
        sample_rate: capture.rate as u32,
        channels: vec![x.iter().map(|&v| v as f32).collect()],
    })?;
    std::fs::write(sidecar(&out), capture.describe())?;
    println!(
        "{}: {:.1} s at {} Hz, peak {:.3}; {} beside it describes it",
        out.display(),
        x.len() as f64 / capture.rate,
        capture.rate,
        peak,
        sidecar(&out).display()
    );
    Ok(())
}

/// A recording's rate, its first channel (the device), and its second
/// when there is one (the interface's loopback).
type Recording = (u32, Vec<f64>, Option<Vec<f64>>);

fn recording(path: &Path) -> Result<Recording, Box<dyn Error>> {
    let audio = wav::read(path)?;
    let channel = |i: usize| audio.channels.get(i).map(|c| c.iter().map(|&v| f64::from(v)).collect::<Vec<f64>>());
    Ok((audio.sample_rate, channel(0).ok_or("no channels")?, channel(1)))
}

/// The measurements and the capture design, from a recording or a circuit.
fn measure(a: &Args) -> Result<(Capture, Measurements, String), Box<dyn Error>> {
    if let Some(path) = a.get("plugin") {
        let capture = Capture::standard(48_000.0, a.number("reference", 0.05)?);
        let config = stave_host::config::HostConfig {
            sample_rate: stave_host::config::SampleRate::constant(48_000.0),
            ..stave_host::config::HostConfig::default()
        };
        let mut opened = stave_host::opened::Opened::open(Path::new(path), config)?;
        let inst = opened.instance();
        let layout = inst.layout();
        if layout.inputs == 0 || layout.outputs == 0 {
            return Err(format!("{path} has no audio input or output").into());
        }
        let latency = inst.latency() as usize;
        let x = capture.samples();
        let mut recorded = Vec::with_capacity(x.len() + latency);
        let block = layout.max_block.max(1);
        let silence = vec![0.0f32; block];
        let mut outs = vec![vec![0.0f32; block]; layout.outputs];
        let feed: Vec<f32> = x.iter().map(|&v| v as f32).chain(std::iter::repeat_n(0.0, latency)).collect();
        for chunk in feed.chunks(block) {
            let n = chunk.len();
            let inputs: Vec<&[f32]> = (0..layout.inputs).map(|c| if c == 0 { chunk } else { &silence[..n] }).collect();
            let mut outputs: Vec<&mut [f32]> = outs.iter_mut().map(|o| &mut o[..n]).collect();
            inst.process(&inputs, &mut outputs, &[]);
            recorded.extend(outs[0][..n].iter().map(|&v| f64::from(v)));
        }
        eprintln!("hosted {path}: {latency} samples of reported latency");
        let m = capture.read(&recorded, latency)?;
        let what = format!("{path} hosted offline at 48 kHz, 0 dB = {:.3} RMS in", capture.reference);
        return Ok((capture, m, what));
    }
    if let Some(name) = a.get("circuit") {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/stave-test/spice");
        let text = std::fs::read_to_string(dir.join(format!("{name}.plan")))
            .map_err(|e| format!("no plan for circuit {name} ({e}); the R17 decks are in crates/stave-test/spice"))?;
        let plan = stave_test::characterise::Plan::parse(&text)?;
        let knee = plan.knee.ok_or("the plan has no knee")?.amplitude;
        let factor: usize = a.number("oversample", 4)?;
        let capture = Capture::standard(48_000.0, knee / std::f64::consts::SQRT_2);
        let deck = stave_test::characterise::deck(name);
        let input = plan.input.clone();
        let rate = 48_000.0 * factor as f64;
        if !factor.is_power_of_two() {
            return Err("--oversample must be a power of two".into());
        }
        eprintln!("measuring {name} at {rate} Hz (0 dB = its knee, {knee:.3e} V peak)…");
        let m = capture.run(factor, move || {
            let mut c = stave_test::characterise::model::circuit(&deck, &input, rate);
            Box::new(move |x| c.tick_f64(x))
        });
        Ok((capture, m, format!("Circuit<Dk> on spice/{name}.cir at {factor}x, 0 dB = its knee")))
    } else {
        let stim = PathBuf::from(a.get("stimulus").ok_or("fit: --stimulus or --circuit")?);
        let rec = PathBuf::from(a.get("capture").ok_or("fit: --capture (the recording)")?);
        let capture = Capture::parse(&std::fs::read_to_string(sidecar(&stim)).map_err(|e| {
            format!("{}: {e} (stave-fit stimulus writes it beside the stimulus)", sidecar(&stim).display())
        })?)?;
        let (rate, recorded, loopback) = recording(&rec)?;
        if f64::from(rate) != capture.rate {
            return Err(format!("the recording is at {rate} Hz, the stimulus at {} Hz", capture.rate).into());
        }
        let (latency, how) = match (a.get("latency"), &loopback) {
            (Some(v), _) => (v.parse().map_err(|_| format!("--latency: {v}"))?, "given"),
            (None, Some(l)) => (capture.align(l, capture.rate as usize), "from the loopback channel"),
            (None, None) => (
                capture.align(&recorded, capture.rate as usize),
                "from the device's output (its group delay included: record a loopback on channel 2 or pass --latency)",
            ),
        };
        eprintln!("the recording is {latency} samples late ({how})");
        let m = capture.read(&recorded, latency)?;
        Ok((capture, m, format!("the recording {} ({latency} samples of latency)", rec.display())))
    }
}

fn fit(a: &Args) -> Result<(), Box<dyn Error>> {
    a.check(&[
        "stimulus", "capture", "circuit", "plugin", "reference", "oversample", "o", "structure", "order", "max-order",
        "knots", "latency", "export-pair",
    ])?;
    let out = PathBuf::from(a.get("o").ok_or("fit: -o MODEL.json")?);
    let (capture, m, what) = measure(a)?;
    let structure = a.get("structure").unwrap_or("wiener-hammerstein");
    let max_order: usize = a.number("max-order", 5)?;
    let mut lines = Vec::new();
    let block: Block = if structure == "parallel" {
        let options = hammerstein::Options {
            order: a.number("order", 5)?,
            branches: hammerstein::Branches::Rational {
                max_order,
                tolerance_db: 0.1,
            },
            ..hammerstein::Options::default()
        };
        let f = hammerstein::fit(&m.sweep, capture.rate, &m.sweep_input, &m.sweep_output, &options)
            .ok_or("the sweep did not separate into harmonics (too short for this order?)")?;
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
            structure: match structure {
                "wiener-hammerstein" => Structure::WienerHammerstein,
                "wiener" => Structure::Wiener,
                "hammerstein" => Structure::Hammerstein,
                other => return Err(format!("--structure {other}: wiener-hammerstein, wiener, hammerstein or parallel").into()),
            },
            max_order,
            knots: a.number("knots", 24)?,
            ..wh::Options::default()
        };
        let f = wh::fit(&m.records, &options).ok_or("no fit (the best linear approximation would not fit)")?;
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
    let mut model = block.clone();
    model.prepare(capture.rate as f32);
    let y: Vec<f64> = m.bursts_input.iter().map(|&x| model.tick_f64(x)).collect();
    lines.push("held-out bursts (simulation error / ERB worst / NMR total):".into());
    for (db, span) in capture.burst_spans() {
        let v = Validation::of(&m.bursts_output[span.clone()], &y[span], capture.rate);
        lines.push(format!("  {db:+5.1} dB: {:.1} dB / {:.2} dB / {:.1} dB", v.error_db, v.erb.max_db, v.nmr.total_db));
    }
    let sections = |l: &Linear| match l {
        Linear::Rational(s) => s.len(),
        Linear::Fir(_) => 0,
    };
    if let Block::WienerHammerstein(w) = &block {
        lines.push(format!("sections: {} before the curve, {} after", sections(w.input()), sections(w.output())));
    }
    let note = format!("fitted by stave-fit to {what}\n{}", lines.join("\n"));
    std::fs::write(&out, block.to_json(&note))?;
    println!("{note}\nwrote {}", out.display());
    if let Some(dir) = a.get("export-pair") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        export_pair(&dir, &capture, &m)?;
    }
    Ok(())
}

/// The measured multisines, sweep and bursts back to back, input and
/// output aligned: a training pair for a network fitted out of tree.
fn export_pair(dir: &Path, capture: &Capture, m: &Measurements) -> Result<(), Box<dyn Error>> {
    let (mut x, mut y) = (Vec::new(), Vec::new());
    for r in &m.records.records {
        x.extend(r.input.iter().map(|&v| v as f32));
        y.extend(r.output.iter().map(|&v| v as f32));
    }
    x.extend(m.sweep_input.iter().map(|&v| v as f32));
    y.extend(m.sweep_output.iter().map(|&v| v as f32));
    x.extend(m.bursts_input.iter().map(|&v| v as f32));
    y.extend(m.bursts_output.iter().map(|&v| v as f32));
    let rate = capture.rate as u32;
    wav::write(&dir.join("input.wav"), &wav::Audio { sample_rate: rate, channels: vec![x] })?;
    wav::write(&dir.join("output.wav"), &wav::Audio { sample_rate: rate, channels: vec![y] })?;
    println!("wrote {} and {}", dir.join("input.wav").display(), dir.join("output.wav").display());
    Ok(())
}
