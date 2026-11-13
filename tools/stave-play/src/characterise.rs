//! `stave-play --characterise CANDIDATE …`: the characterisation (DSP.md
//! R17–R19) of each candidate as a black box, at each rate, set side by
//! side as a markdown table — small-signal response, compression and
//! harmonics against level, aliasing floor and ANMR, Rnonlin, latency,
//! tail and CPU cost. Headless.
//!
//! A candidate is the voicing tools' spec (`stage:NAME`, a `.vst3`, an
//! Audio Unit as `au:NAME` or its `.component`, then `;name=value`
//! settings or `;preset=NAME`). Each contender is measured at levels
//! relative to its own knee, so a plugin's full scale and a circuit's
//! volts need no common calibration. A third-party plugin here is a
//! benchmark: keep its tables out of the repository.

use std::path::PathBuf;

use stave_host::config::{HostConfig, SampleRate};
use stave_host::instance::apply_preset;
use stave_host::opened::Opened;
use stave_test::characterise::black_box::{self, BlackBox};
use stave_test::characterise::Tier;

use crate::voicing::Candidate;

/// What `--characterise` runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub candidates: Vec<String>,
    pub rates: Vec<f64>,
    pub block: usize,
    pub tier: Tier,
    pub out: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            candidates: Vec::new(),
            rates: vec![44_100.0, 48_000.0],
            block: 64,
            tier: Tier::T1,
            out: None,
        }
    }
}

/// Open `spec` at `rate` and apply its preset and settings; the
/// resolved settings as text.
fn open(spec: &str, rate: f64, block: usize) -> Result<(String, Box<dyn stave_host::instance::Instance + Send>, String), String> {
    let (c, _) = Candidate::parse(spec).map_err(|e| format!("{spec}: {e}"))?;
    let crate::voicing::candidate::Source::Plugin { path } = &c.source else {
        return Err(format!("{spec}: only a plugin or stage can be characterised"));
    };
    let config = HostConfig {
        sample_rate: SampleRate::new(rate).map_err(|e| e.to_string())?,
        max_block: std::num::NonZeroUsize::new(block).ok_or("block 0")?,
        ..HostConfig::default()
    };
    let mut inst = Opened::open(path, config).map_err(|e| format!("{spec}: {}", chain(&e)))?.into_send();
    if let Some(p) = &c.preset {
        apply_preset(inst.as_mut(), p).map_err(|e| format!("{spec}: {e}"))?;
    }
    for s in &c.settings {
        inst.set_param(&s.name, s.value).map_err(|e| format!("{spec}: {}: {e}", s.name))?;
    }
    let settings: Vec<String> = inst
        .params()
        .iter()
        .filter(|p| !p.read_only)
        .map(|p| match p.step_names.get(p.value.round().max(0.0) as usize) {
            Some(n) if p.step_count > 0 => format!("{}={n}", p.name),
            _ => format!("{}={}{}", p.name, crate::voicing::candidate::trim_number(p.value), p.unit),
        })
        .collect();
    Ok((c.label, inst, settings.join(", ")))
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(c) = src {
        s.push_str(&format!(": {c}"));
        src = c.source();
    }
    s
}

/// Run every candidate at every rate; the report as markdown.
pub fn run(o: &Options) -> Result<String, String> {
    let mut report = String::new();
    for &rate in &o.rates {
        let mut done: Vec<(String, BlackBox, String)> = Vec::new();
        for spec in &o.candidates {
            eprintln!("characterise: {spec} at {rate} Hz ({:?})", o.tier);
            let started = std::time::Instant::now();
            let (label, inst, settings) = open(spec, rate, o.block)?;
            let b = black_box::characterise(inst, rate, o.block, o.tier);
            eprintln!("  done in {:.1} s", started.elapsed().as_secs_f64());
            done.push((label, b, settings));
        }
        report.push_str(&format!("### {:.1} kHz ({:?})\n\n", rate / 1e3, o.tier));
        let rows: Vec<(String, &BlackBox)> = done.iter().map(|(l, b, _)| (l.clone(), b)).collect();
        report.push_str(&black_box::side_by_side(&rows));
        report.push('\n');
        for (label, _, settings) in &done {
            report.push_str(&format!("- `{label}`: {settings}\n"));
        }
        report.push('\n');
    }
    if let Some(path) = &o.out {
        std::fs::write(path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(report)
}

/// `44100,48000` → rates.
pub fn parse_rates(v: &str) -> Option<Vec<f64>> {
    v.split(',')
        .map(|r| r.trim().parse::<f64>().ok().filter(|r| SampleRate::new(*r).is_ok()))
        .collect::<Option<Vec<f64>>>()
        .filter(|r| !r.is_empty())
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

    /// Two catalog stages side by side, end to end.
    #[test]
    fn characterises_stages_side_by_side() {
        let o = Options {
            candidates: vec!["stage:coupler".into(), "stage:clipper/diode;label=diode".into()],
            rates: vec![48_000.0],
            tier: Tier::T0,
            ..Options::default()
        };
        let md = run(&o).unwrap();
        assert!(md.contains("| | stage:coupler | diode |"), "{md}");
        assert!(md.contains("knee @ 1 kHz"), "{md}");
        assert!(run(&Options { candidates: vec!["dry".into()], ..o }).is_err());
    }
}
