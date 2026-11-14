//! `characterise`: each target as a black box (DSP.md R17–R19), at each
//! rate, side by side — small-signal response, compression and
//! harmonics against level, aliasing floor and ANMR, Rnonlin, latency,
//! tail and CPU cost. Each contender is measured at levels relative to
//! its own knee, so a plugin's full scale and a circuit's volts need no
//! common calibration. A third-party plugin here is a benchmark: keep
//! its tables out of the repository.

use std::num::NonZeroUsize;
use std::path::Path;

use serde_json::json;
use stave_host::config::{HostConfig, SampleRate};
use stave_host::opened::Opened;
use stave_test::characterise::black_box::{self, BlackBox};
use stave_test::characterise::Tier;

use crate::command::{causes, Artefact, CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};
use crate::target::Target;

pub const CHARACTERISE: CommandInfo = CommandInfo {
    id: "characterise",
    title: "Characterise plugins side by side",
    summary: "Black-box characterisation (DSP.md R17-R19) of each target, side by side per rate",
    description: "Measure each target (a plugin, an Audio Unit or a stage, with ;name=value settings or \
                  ;preset=NAME) as a black box at each rate: its knee, the response, compression and \
                  harmonics against level re the knee, the aliasing floor and ANMR, Rnonlin, latency, \
                  tail and CPU cost. The report is a markdown table per rate, contenders as columns. \
                  T0 is the quick tier; T1 (default) adds levels and tones.",
    inputs: &[
        Field::positional("targets", Kind::Strings, "The targets to measure (one or more)"),
        Field::flag("rates", Kind::String, "Rates, Hz, comma-separated").default("44100,48000"),
        Field::flag("tier", Kind::Choice(&["t0", "t1"]), "t0 quick, t1 thorough").default("t1"),
        Field::flag("process_block", Kind::Integer, "Frames per process call").default("64"),
        Field::flag("report", Kind::String, "Also write the markdown report here"),
    ],
    outputs: &[
        Field::flag("rates", Kind::Json, "The rates measured").required(),
        Field::flag("tier", Kind::String, "t0 or t1").required(),
        Field::flag("contenders", Kind::Json, "Per rate and target: label and the settings it ran with").required(),
        Field::flag("report", Kind::String, "The side-by-side tables, markdown").required(),
    ],
    long_running: true,
};

/// `44100,48000` → rates.
pub fn parse_rates(v: &str) -> Option<Vec<f64>> {
    v.split(',')
        .map(|r| r.trim().parse::<f64>().ok().filter(|r| SampleRate::new(*r).is_ok()))
        .collect::<Option<Vec<f64>>>()
        .filter(|r| !r.is_empty())
}

/// Open a target at `rate` with its preset and settings; its label, the
/// instance, and the resolved settings as text.
fn open(spec: &str, rate: f64, block: usize) -> Result<(String, Box<dyn stave_host::instance::Instance + Send>, String), CommandError> {
    let t = Target::parse(spec)?;
    if t.session().is_some() {
        return Err(CommandError::Usage(format!("{spec}: characterise opens its own instances; name the plugin or stage")));
    }
    let config = HostConfig {
        sample_rate: SampleRate::new(rate).map_err(|e| CommandError::Usage(e.to_string()))?,
        max_block: NonZeroUsize::new(block).ok_or_else(|| CommandError::Usage("process_block: at least 1".into()))?,
        ..HostConfig::default()
    };
    let mut inst = Opened::open(Path::new(&t.head), config)
        .map_err(|e| CommandError::Failed(format!("{spec}: {}", causes(&e))))?
        .into_send();
    t.apply(inst.as_mut()).map_err(|e| CommandError::Failed(format!("{spec}: {e}")))?;
    let settings: Vec<String> = inst
        .params()
        .iter()
        .filter(|p| !p.read_only)
        .map(|p| match p.step_names.get(p.value.round().max(0.0) as usize) {
            Some(n) if p.step_count > 0 => format!("{}={n}", p.name),
            _ => format!("{}={}{}", p.name, trim_number(p.value), p.unit),
        })
        .collect();
    let label = t.label.clone().unwrap_or_else(|| default_label(&t));
    Ok((label, inst, settings.join(", ")))
}

/// The head and its settings, briefly ("stage:clipper/diode threshold=0.6").
fn default_label(t: &Target) -> String {
    let mut parts = vec![match t.head.strip_prefix("stage:").or_else(|| t.head.strip_prefix("au:")) {
        Some(_) => t.head.clone(),
        None => Path::new(&t.head)
            .file_stem()
            .map_or_else(|| t.head.clone(), |s| s.to_string_lossy().into_owned()),
    }];
    if let Some(p) = &t.preset {
        parts.push(format!("[{p}]"));
    }
    parts.extend(t.settings.iter().map(|(n, v)| format!("{n}={}", trim_number(*v))));
    parts.join(" ")
}

/// `0.5`, `3`, `-12.25`: no trailing zeros.
fn trim_number(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

pub fn characterise(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let targets = input.strings("targets");
    if targets.is_empty() {
        return Err(CommandError::Usage("characterise: name at least one target".into()));
    }
    let rates_text = input.str("rates")?;
    let rates = parse_rates(rates_text).ok_or_else(|| CommandError::Usage(format!("rates: '{rates_text}' is not rates in Hz")))?;
    let tier = match input.str("tier")? {
        "t0" => Tier::T0,
        _ => Tier::T1,
    };
    let block = input.integer("process_block").unwrap_or(64) as usize;
    let total = (rates.len() * targets.len()) as f64;
    let mut report = String::new();
    let mut contenders = Vec::new();
    let mut step = 0.0;
    for &rate in &rates {
        let mut done: Vec<(String, BlackBox, String)> = Vec::new();
        for spec in &targets {
            cx.checkpoint()?;
            cx.progress(step, Some(total), format!("{spec} at {rate} Hz ({tier:?})"));
            let (label, inst, settings) = open(spec, rate, block)?;
            done.push((label, black_box::characterise(inst, rate, block, tier), settings));
            step += 1.0;
        }
        report.push_str(&format!("### {:.1} kHz ({tier:?})\n\n", rate / 1e3));
        let rows: Vec<(String, &BlackBox)> = done.iter().map(|(l, b, _)| (l.clone(), b)).collect();
        report.push_str(&black_box::side_by_side(&rows));
        report.push('\n');
        for (label, _, settings) in &done {
            report.push_str(&format!("- `{label}`: {settings}\n"));
            contenders.push(json!({ "rate": rate, "label": label, "settings": settings }));
        }
        report.push('\n');
    }
    cx.progress(total, Some(total), "done");
    let mut outcome = Outcome::ok(
        json!({ "rates": rates, "tier": input.str("tier")?, "contenders": contenders, "report": report }),
        report.clone(),
    )?;
    if let Some(path) = input.path("report") {
        std::fs::write(&path, &report).map_err(|e| CommandError::Failed(format!("{}: {e}", path.display())))?;
        outcome = outcome.with(Artefact::new(path, "text/markdown", "report"));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_parse_or_fail() {
        assert_eq!(parse_rates("44100, 48000"), Some(vec![44_100.0, 48_000.0]));
        assert_eq!(parse_rates("44100,x"), None);
        assert_eq!(parse_rates("0"), None);
    }

    #[test]
    fn labels_are_brief() {
        let t = Target::parse("/x/Some Amp.vst3;preset=Hot;gain=2.50").unwrap();
        assert_eq!(default_label(&t), "Some Amp [Hot] gain=2.5");
        assert_eq!(default_label(&Target::parse("stage:coupler").unwrap()), "stage:coupler");
    }
}
