//! `stave-play --characterise CANDIDATE …` — deprecated for `stave
//! characterise`, which this runs in-process (the command lives in
//! `stave_tools::commands::characterise`): each candidate measured as a
//! black box (DSP.md R17–R19) at each rate, side by side as markdown.

use std::path::PathBuf;

use stave_test::characterise::Tier;
use stave_tools::{Cancel, Context, Progress, Workspace};

pub use stave_tools::commands::characterise::parse_rates;

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

/// Run every candidate at every rate; the report as markdown.
pub fn run(o: &Options) -> Result<String, String> {
    eprintln!("stave-play --characterise is deprecated: use `stave characterise`");
    let rates: Vec<String> = o.rates.iter().map(f64::to_string).collect();
    let mut input = serde_json::json!({
        "targets": o.candidates,
        "rates": rates.join(","),
        "tier": if o.tier == Tier::T0 { "t0" } else { "t1" },
        "process_block": o.block,
    });
    if let Some(p) = &o.out {
        input["report"] = serde_json::json!(p.display().to_string());
    }
    let mut ws = Workspace::default();
    let mut progress = |p: &Progress| eprintln!("characterise: {}", p.message);
    let mut cx = Context::new(&mut ws, &mut progress, Cancel::default());
    stave_tools::registry()
        .run("characterise", input, &mut cx)
        .map(|o| o.text)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
