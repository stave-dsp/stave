//! Running a plugin on a signal: `bench` (the timed run's cost,
//! latency and output, measured) and `render` (its output to a WAV).

use std::path::Path;

use serde_json::json;
use stave_host::bench::{self, text, BenchConfig, Stimulus};
use stave_host::note::Note;
use stave_host::wav;
use stave_measure::loudness;

use crate::command::{causes, Artefact, CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};
use crate::target::{hold, Target, BLOCK, PRESET, RATE, SET, TARGET};

const SECONDS: Field = Field::flag("seconds", Kind::Number, "Length of the timed run, seconds").default("3");
const SIGNAL: Field = Field::flag(
    "signal",
    Kind::String,
    "sine[:FREQ[:DBFS]] | impulse | silence | noise[:DBFS] | sweep[:F0:F1[:DBFS]] | wav:PATH",
)
.default("sine:1000:-12");
const NOTES: Field = Field::flag(
    "notes",
    Kind::Strings,
    "Notes, KEY@ON:OFF[:VELOCITY] in seconds, velocity 0..1 (comma-separated or repeated), e.g. 60@0:1.5,64@0.5:1.5:0.4",
);
const MEASURE: Field = Field::flag(
    "measure",
    Kind::String,
    "Level measures, comma-separated: peak, true-peak, rms, lufs-m, lufs-s, lufs-i",
)
.default("peak,true-peak,rms,lufs-i");
const ANALYSIS_WINDOW: Field =
    Field::flag("analysis_window", Kind::Integer, "Tone-analysis window, frames (at least 64)").default("32768");

pub const BENCH: CommandInfo = CommandInfo {
    id: "bench",
    title: "Bench a plugin",
    summary: "CPU cost, latency and output measures of a timed run on a signal",
    description: "Run the target on a signal (and notes, for an instrument) and measure it: CPU per block \
                  against the real-time budget, reported and measured latency, output levels, tone \
                  analysis (THD, harmonics) for a sine, stereo image, DC, the step response, its taps and \
                  each phase's time. --out also writes the timed run's output as a 32-bit float WAV.",
    inputs: &[
        TARGET,
        RATE,
        BLOCK,
        SET,
        PRESET,
        SECONDS,
        SIGNAL,
        NOTES,
        MEASURE,
        ANALYSIS_WINDOW,
        Field::flag("out", Kind::String, "Write the timed run's output here (32-bit float WAV)"),
    ],
    outputs: &[
        Field::flag("sample_rate", Kind::Number, "Hz").required(),
        Field::flag("block", Kind::Integer, "Frames per process call").required(),
        Field::flag("stimulus", Kind::Json, "The signal run").required(),
        Field::flag("cpu", Kind::Json, "Cost per block and per sample, % of real time"),
        Field::flag("latency", Kind::Json, "Reported and measured latency").required(),
        Field::flag("output", Kind::Json, "Levels, tone analysis, stereo image, DC, step response").required(),
        Field::flag("taps", Kind::Json, "The plugin's measurements").required(),
        Field::flag("phases", Kind::Json, "Each phase's frames and wall time").required(),
        Field::flag("warnings", Kind::Strings, "What may make the numbers misleading").required(),
    ],
    long_running: true,
};

/// The bench configuration an input asks for.
fn config(input: &Input) -> Result<BenchConfig, CommandError> {
    let usage = |m: String| CommandError::Usage(m);
    let mut c = BenchConfig::default();
    if let Some(s) = input.number("seconds") {
        if s <= 0.0 {
            return Err(usage(format!("seconds: {s} is not positive")));
        }
        c.seconds = s;
    }
    if let Some(s) = input.opt_str("signal") {
        c.stimulus = Stimulus::parse(s).map_err(|e| usage(format!("signal: {}", causes(&e))))?;
    }
    for n in input.strings("notes") {
        c.notes.extend(parse_notes(&n)?);
    }
    if let Some(m) = input.opt_str("measure") {
        let names: Vec<String> = m.split(',').map(|n| n.trim().to_string()).collect();
        loudness::by_names(&names).map_err(|e| usage(format!("measure: {}", causes(&e))))?;
        c.measures = names;
    }
    if let Some(w) = input.integer("analysis_window") {
        if w < 64 {
            return Err(usage(format!("analysis_window: {w} is under 64 frames")));
        }
        c.analysis_window = w as usize;
    }
    Ok(c)
}

pub fn bench(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut cfg = config(input)?;
    let out = input.path("out");
    cfg.capture = out.is_some();
    let title = Target::from_input(input)?.display();
    cx.progress(0.0, None, format!("running {title}"));
    let mut held = hold(input, cx.workspace)?;
    let run = bench::run(held.instance(), &cfg).map_err(|e| CommandError::from_error(&e))?;
    let mut outcome = Outcome::ok(&run.report, text::bench(&run.report, &title))?;
    if let (Some(path), Some(channels)) = (out, run.capture) {
        write_wav(&path, run.report.sample_rate, channels)?;
        outcome = outcome.with(Artefact::new(path, "audio/wav", "output"));
    }
    Ok(outcome)
}

pub const RENDER: CommandInfo = CommandInfo {
    id: "render",
    title: "Render to WAV",
    summary: "Process a signal (or a WAV) through the target and write its output",
    description: "Run the target on a signal — a WAV file as wav:PATH, or a generated sine, sweep, noise or \
                  impulse — with notes for an instrument, and write the output as a 32-bit float WAV at \
                  the target's rate. Reports the output's levels.",
    inputs: &[
        TARGET,
        Field::flag("out", Kind::String, "The WAV to write").required(),
        RATE,
        BLOCK,
        SET,
        PRESET,
        SECONDS,
        SIGNAL,
        NOTES,
        MEASURE,
    ],
    outputs: &[
        Field::flag("out", Kind::String, "The WAV written").required(),
        Field::flag("sample_rate", Kind::Number, "Hz").required(),
        Field::flag("channels", Kind::Integer, "Output channels").required(),
        Field::flag("frames", Kind::Integer, "Frames per channel").required(),
        Field::flag("levels", Kind::Json, "The output's level measures").required(),
        Field::flag("non_finite_samples", Kind::Integer, "NaN or infinite samples in the output").required(),
    ],
    long_running: true,
};

pub fn render(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut cfg = config(input)?;
    cfg.capture = true;
    let out = input.path("out").ok_or_else(|| CommandError::Usage("render: out is required".into()))?;
    cx.progress(0.0, None, format!("rendering {}", Target::from_input(input)?.display()));
    let mut held = hold(input, cx.workspace)?;
    let run = bench::run(held.instance(), &cfg).map_err(|e| CommandError::from_error(&e))?;
    let channels = run.capture.unwrap_or_default();
    let frames = channels.first().map_or(0, Vec::len);
    let n = channels.len();
    let rate = run.report.sample_rate;
    write_wav(&out, rate, channels)?;
    let levels = &run.report.output.levels;
    let mut text = format!("wrote {} ({n} ch, {frames} frames at {rate} Hz)\n", out.display());
    for r in levels {
        let cell = |v: Option<f64>| v.map_or_else(|| "—".to_string(), |v| format!("{v:.2}"));
        let channels: Vec<String> = r.channels.iter().map(|&v| cell(v)).collect();
        text.push_str(&format!("  {:<10} {:>8} {}  (channels {})\n", r.measure, cell(r.program), r.unit, channels.join(" ")));
    }
    Ok(Outcome::ok(
        json!({
            "out": out, "sample_rate": rate, "channels": n, "frames": frames, "levels": levels,
            "non_finite_samples": run.report.output.non_finite_samples,
        }),
        text,
    )?
    .with(Artefact::new(out, "audio/wav", "output")))
}

fn write_wav(path: &Path, hz: f64, channels: Vec<Vec<f32>>) -> Result<(), CommandError> {
    if hz.fract() != 0.0 || hz > f64::from(u32::MAX) {
        return Err(CommandError::Usage(format!("a WAV header holds whole-number rates; {hz} Hz is not")));
    }
    let audio = wav::Audio {
        sample_rate: hz as u32,
        channels,
    };
    wav::write(path, &audio).map_err(|e| CommandError::from_error(&e))
}

/// `KEY@ON:OFF[:VELOCITY][,…]`, seconds; velocity 0..=1, default MIDI 100.
pub fn parse_notes(s: &str) -> Result<Vec<Note>, CommandError> {
    s.split(',').filter(|p| !p.trim().is_empty()).map(parse_note).collect()
}

fn parse_note(part: &str) -> Result<Note, CommandError> {
    let malformed = |problem: &str| CommandError::Usage(format!("notes: '{part}' {problem}"));
    let shape = || malformed("is not KEY@ON:OFF[:VELOCITY]");
    let (key, times) = part.split_once('@').ok_or_else(shape)?;
    let mut fields = times.split(':').map(str::trim);
    let (Some(on), Some(off)) = (fields.next(), fields.next()) else {
        return Err(shape());
    };
    let velocity = match fields.next() {
        None => Note::DEFAULT_VELOCITY,
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or_else(|| malformed("has a velocity outside 0..=1"))?,
    };
    if fields.next().is_some() {
        return Err(shape());
    }
    let note = Note {
        key: key.trim().parse().map_err(|_| shape())?,
        velocity,
        on_s: on.parse().map_err(|_| shape())?,
        off_s: off.parse().map_err(|_| shape())?,
    };
    if note.off_s < note.on_s {
        return Err(malformed("ends before it starts"));
    }
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_take_an_optional_velocity() {
        let n = parse_notes("60@0:1,64@0.5:1:0.25").unwrap();
        assert_eq!(n[0].velocity, Note::DEFAULT_VELOCITY);
        assert_eq!((n[1].key, n[1].velocity, n[1].on_s), (64, 0.25, 0.5));
        for bad in ["60@0:1:2", "60@1", "60@2:1", "x@0:1", "60@0:1:0.5:9"] {
            assert!(matches!(parse_notes(bad), Err(CommandError::Usage(m)) if m.contains("notes")), "{bad}");
        }
    }
}
